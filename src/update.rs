use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const API_URL: &str = "https://api.github.com/repos/Harsh-2002/MDX/releases/latest";
const DOWNLOAD_BASE: &str = "https://github.com/Harsh-2002/MDX/releases/download";

fn http_agent() -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(60)))
        .build();
    config.into()
}

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
    let url = format!("{}/{}/mdx-{}.tar.gz", DOWNLOAD_BASE, tag, target);

    let temp_dir = std::env::temp_dir().join(format!("mdx-update-{}", std::process::id()));
    fs::create_dir_all(&temp_dir)?;

    // Ensure cleanup on all exit paths
    let result = download_and_install(&url, &temp_dir, &tag);

    // Phase 4: Cleanup temp dir (best-effort)
    let _ = fs::remove_dir_all(&temp_dir);

    result?;

    // Regenerate shell completions for the new binary (best-effort)
    regenerate_completions();

    println!("  Updated mdx: v{} → v{}", CURRENT_VERSION, latest);
    Ok(())
}

fn fetch_latest_tag() -> Result<String, Box<dyn std::error::Error>> {
    eprintln!("  Checking for updates...");
    let agent = http_agent();
    let resp = agent
        .get(API_URL)
        .header("User-Agent", "mdx-cli")
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
        #[cfg(target_env = "musl")]
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-musl"),
        #[cfg(not(target_env = "musl"))]
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu"),
        #[cfg(target_env = "musl")]
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-musl"),
        #[cfg(not(target_env = "musl"))]
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("linux", "arm") => Ok("armv7-unknown-linux-gnueabihf"),
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc"),
        ("windows", "aarch64") => Ok("aarch64-pc-windows-msvc"),
        _ => Err(format!(
            "Unsupported platform: {}/{}. Pre-built binaries available for: \
             linux (x86_64, aarch64, armv7), macOS (x86_64, aarch64), Windows (x86_64, aarch64)",
            os, arch
        )
        .into()),
    }
}

fn download_and_install(
    url: &str,
    temp_dir: &Path,
    tag: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Download tarball
    eprintln!("  Downloading {}...", tag);
    let agent = http_agent();
    let resp = agent
        .get(url)
        .header("User-Agent", "mdx-cli")
        .call()
        .map_err(|e| format!("Failed to download release: {}", e))?;

    let tarball_path = temp_dir.join("mdx.tar.gz");
    let mut body = resp.into_body();
    let mut file = fs::File::create(&tarball_path)?;
    std::io::copy(&mut body.as_reader(), &mut file)?;
    file.flush()?;
    drop(file);

    // Extract (use -xzf with leading dash for Windows bsdtar compatibility)
    let tarball_str = tarball_path
        .to_str()
        .ok_or("Temp directory path contains invalid characters")?;
    let status = Command::new("tar")
        .args(["-xzf", tarball_str, "-C"])
        .arg(temp_dir)
        .status()
        .map_err(|e| format!("Failed to run tar: {}", e))?;

    if !status.success() {
        return Err("Failed to extract update archive".into());
    }

    let binary_name = format!("mdx{}", std::env::consts::EXE_SUFFIX);
    let new_binary = temp_dir.join(&binary_name);
    if !new_binary.exists() {
        return Err(format!(
            "Downloaded archive does not contain '{}' binary",
            binary_name
        )
        .into());
    }

    // Pre-verify the new binary
    let output = Command::new(&new_binary)
        .arg("--version")
        .output()
        .map_err(|e| format!("Failed to verify downloaded binary: {}", e))?;

    if !output.status.success() {
        return Err("Downloaded binary is invalid (--version check failed)".into());
    }

    // Phase 3: Binary replacement
    let current_exe = std::env::current_exe()?;
    let exe_path = fs::canonicalize(&current_exe)?;
    let exe_dir = exe_path
        .parent()
        .ok_or("Could not determine binary directory")?;

    let staging_path = exe_dir.join(format!("mdx.update.tmp{}", std::env::consts::EXE_SUFFIX));

    // Copy new binary to staging location (same filesystem for atomic rename)
    fs::copy(&new_binary, &staging_path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            #[cfg(unix)]
            {
                format!(
                    "Permission denied writing to {}. Try: sudo mdx update",
                    exe_dir.display()
                )
            }
            #[cfg(windows)]
            {
                format!(
                    "Permission denied writing to {}. Try running as Administrator",
                    exe_dir.display()
                )
            }
            #[cfg(not(any(unix, windows)))]
            {
                format!("Permission denied writing to {}", exe_dir.display())
            }
        } else {
            format!("Failed to stage update: {}", e)
        }
    })?;

    #[cfg(unix)]
    {
        // Set executable permissions
        fs::set_permissions(&staging_path, fs::Permissions::from_mode(0o755))?;

        // Atomic swap
        if let Err(e) = fs::rename(&staging_path, &exe_path) {
            let _ = fs::remove_file(&staging_path);
            return Err(format!("Failed to replace binary: {}", e).into());
        }
    }

    #[cfg(windows)]
    {
        // Windows locks running executables, but allows renaming them.
        // Rename the running exe out of the way, then move the new one in.
        let old_path = exe_dir.join("mdx.old.exe");

        // Clean up any leftover from a previous update
        let _ = fs::remove_file(&old_path);

        // Rename running binary: mdx.exe -> mdx.old.exe
        if let Err(e) = fs::rename(&exe_path, &old_path) {
            let _ = fs::remove_file(&staging_path);
            return Err(format!("Failed to rename running binary: {}", e).into());
        }

        // Move staged binary into place: mdx.update.tmp.exe -> mdx.exe
        if let Err(e) = fs::rename(&staging_path, &exe_path) {
            // Try to restore the old binary
            let _ = fs::rename(&old_path, &exe_path);
            return Err(format!("Failed to install new binary: {}", e).into());
        }

        // Try to delete the old binary (may fail if still locked — that's OK,
        // cleanup_old_binary() will get it on next launch)
        let _ = fs::remove_file(&old_path);
    }

    Ok(())
}

/// Regenerate shell completions after a self-update.
/// Best-effort — errors are silently ignored so they never block an update.
fn regenerate_completions() {
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(_) => return,
    };
    let exe_dir = match exe.parent() {
        Some(d) => d,
        None => return,
    };
    let new_binary = exe_dir.join(format!("mdx{}", std::env::consts::EXE_SUFFIX));
    if !new_binary.exists() {
        return;
    }

    #[cfg(unix)]
    {
        let home = match std::env::var("HOME") {
            Ok(h) => PathBuf::from(h),
            Err(_) => return,
        };
        let shell = std::env::var("SHELL").unwrap_or_default();
        let shell_name = std::path::Path::new(&shell)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        let wrote = match shell_name.as_str() {
            "bash" => {
                let dir = home.join(".local/share/bash-completion/completions");
                let _ = fs::create_dir_all(&dir);
                let _ = fs::remove_file(dir.join("md")); // clean up old v4
                write_completion(&new_binary, "bash", &dir.join("mdx"))
            }
            "zsh" => {
                let dir = home.join(".local/share/zsh/site-functions");
                let _ = fs::create_dir_all(&dir);
                let _ = fs::remove_file(dir.join("_md")); // clean up old v4
                write_completion(&new_binary, "zsh", &dir.join("_mdx"))
            }
            "fish" => {
                let dir = home.join(".config/fish/completions");
                let _ = fs::create_dir_all(&dir);
                let _ = fs::remove_file(dir.join("md.fish")); // clean up old v4
                write_completion(&new_binary, "fish", &dir.join("mdx.fish"))
            }
            _ => false,
        };

        if wrote {
            eprintln!("  Shell completions updated.");
        }
    }

    #[cfg(windows)]
    {
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            let local = PathBuf::from(&local_app_data);

            // Clean up old v4 'md' completions
            let old_dir = local.join("md");
            if old_dir.exists() {
                let _ = fs::remove_dir_all(&old_dir);
            }

            let comp_dir = local.join("mdx").join("completions");
            let _ = fs::create_dir_all(&comp_dir);

            if write_completion(&new_binary, "powershell", &comp_dir.join("mdx.ps1")) {
                eprintln!("  Shell completions updated.");
            }
        }
    }
}

/// Run `mdx completions <shell>` and write the output to `dest`. Returns true on success.
fn write_completion(binary: &std::path::Path, shell: &str, dest: &std::path::Path) -> bool {
    match Command::new(binary).args(["completions", shell]).output() {
        Ok(output) if output.status.success() && !output.stdout.is_empty() => {
            fs::write(dest, &output.stdout).is_ok()
        }
        _ => false,
    }
}

/// Clean up leftover `mdx.old.exe` from a previous update.
/// Called at startup from main(). Best-effort — silently ignores errors.
#[cfg(windows)]
pub fn cleanup_old_binary() {
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(exe_dir) = current_exe.parent() {
            let old = exe_dir.join("mdx.old.exe");
            if old.exists() {
                let _ = fs::remove_file(&old);
            }
        }
    }
}
