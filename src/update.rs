use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const API_URL: &str = "https://api.github.com/repos/Harsh-2002/MD/releases/latest";
const DOWNLOAD_BASE: &str = "https://github.com/Harsh-2002/MD/releases/download";

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Phase 1: Check if update is needed
    let tag = fetch_latest_tag()?;
    let latest = tag.strip_prefix('v').unwrap_or(&tag);

    if latest == CURRENT_VERSION {
        println!("  Already on the latest version (v{})", CURRENT_VERSION);
        return Ok(());
    }

    println!(
        "  New version available: v{} → v{}",
        CURRENT_VERSION, latest
    );

    // Phase 2: Download and verify
    let target = detect_target()?;
    let url = format!("{}/{}/md-{}.tar.gz", DOWNLOAD_BASE, tag, target);

    let temp_dir = std::env::temp_dir().join(format!("md-update-{}", std::process::id()));
    fs::create_dir_all(&temp_dir)?;

    // Ensure cleanup on all exit paths
    let result = download_and_install(&url, &temp_dir, &tag);

    // Phase 4: Cleanup temp dir (best-effort)
    let _ = fs::remove_dir_all(&temp_dir);

    result?;

    println!("  Updated md: v{} → v{}", CURRENT_VERSION, latest);
    Ok(())
}

fn fetch_latest_tag() -> Result<String, Box<dyn std::error::Error>> {
    eprintln!("  Checking for updates...");
    let resp = ureq::get(API_URL)
        .header("User-Agent", "md-cli")
        .call()
        .map_err(|e| format!("Failed to check for updates: {}", e))?;

    let body = resp
        .into_body()
        .read_to_string()
        .map_err(|e| format!("Failed to read response: {}", e))?;

    // Extract tag_name without serde — find "tag_name":"vX.Y.Z"
    let tag = extract_json_string(&body, "tag_name")
        .ok_or("Could not parse latest version from GitHub API response")?;

    Ok(tag)
}

fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{}\"", key);
    let key_pos = json.find(&pattern)?;
    let after_key = &json[key_pos + pattern.len()..];
    // Skip whitespace and colon
    let after_colon = after_key.trim_start().strip_prefix(':')?;
    let after_colon = after_colon.trim_start();
    // Expect a quoted string
    let after_quote = after_colon.strip_prefix('"')?;
    let end_quote = after_quote.find('"')?;
    Some(after_quote[..end_quote].to_string())
}

fn detect_target() -> Result<&'static str, Box<dyn std::error::Error>> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    match (os, arch) {
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu"),
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        _ => Err(format!(
            "Unsupported platform: {}/{}. Pre-built binaries available for: \
             linux (x86_64, aarch64), macOS (x86_64, aarch64)",
            os, arch
        )
        .into()),
    }
}

fn download_and_install(
    url: &str,
    temp_dir: &PathBuf,
    tag: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Download tarball
    eprintln!("  Downloading {}...", tag);
    let resp = ureq::get(url)
        .header("User-Agent", "md-cli")
        .call()
        .map_err(|e| format!("Failed to download release: {}", e))?;

    let tarball_path = temp_dir.join("md.tar.gz");
    let mut body = resp.into_body();
    let mut file = fs::File::create(&tarball_path)?;
    std::io::copy(&mut body.as_reader(), &mut file)?;
    file.flush()?;
    drop(file);

    // Extract
    let status = Command::new("tar")
        .args(["xzf", tarball_path.to_str().unwrap(), "-C"])
        .arg(temp_dir)
        .status()
        .map_err(|e| format!("Failed to run tar: {}", e))?;

    if !status.success() {
        return Err("Failed to extract update archive".into());
    }

    let new_binary = temp_dir.join("md");
    if !new_binary.exists() {
        return Err("Downloaded archive does not contain 'md' binary".into());
    }

    // Pre-verify the new binary
    let output = Command::new(&new_binary)
        .arg("--version")
        .output()
        .map_err(|e| format!("Failed to verify downloaded binary: {}", e))?;

    if !output.status.success() {
        return Err("Downloaded binary is invalid (--version check failed)".into());
    }

    // Phase 3: Atomic binary replacement
    let current_exe = std::env::current_exe()?;
    let exe_path = fs::canonicalize(&current_exe)?;
    let exe_dir = exe_path
        .parent()
        .ok_or("Could not determine binary directory")?;

    let staging_path = exe_dir.join("md.update.tmp");

    // Copy new binary to staging location (same filesystem for atomic rename)
    fs::copy(&new_binary, &staging_path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            format!(
                "Permission denied writing to {}. Try: sudo md update",
                exe_dir.display()
            )
        } else {
            format!("Failed to stage update: {}", e)
        }
    })?;

    // Set executable permissions
    fs::set_permissions(&staging_path, fs::Permissions::from_mode(0o755))?;

    // Atomic swap
    if let Err(e) = fs::rename(&staging_path, &exe_path) {
        // Clean up staging file on failure
        let _ = fs::remove_file(&staging_path);
        return Err(format!("Failed to replace binary: {}", e).into());
    }

    Ok(())
}
