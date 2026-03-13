use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use clap::CommandFactory;

use crate::cli::Args;

/// Print shell completions to stdout.
pub fn generate(shell_name: &str) {
    let shell: clap_complete::Shell = match shell_name.to_lowercase().as_str() {
        "bash" => clap_complete::Shell::Bash,
        "zsh" => clap_complete::Shell::Zsh,
        "fish" => clap_complete::Shell::Fish,
        "powershell" | "pwsh" => clap_complete::Shell::PowerShell,
        _ => {
            eprintln!(
                "Unknown shell: {}. Supported: bash, zsh, fish, powershell",
                shell_name
            );
            std::process::exit(1);
        }
    };

    let mut cmd = Args::command();
    clap_complete::generate(shell, &mut cmd, "mdx", &mut io::stdout());
}

/// Auto-detect the user's shell, generate completions, write to the correct
/// location, and update shell config if needed.
pub fn install() -> Result<(), Box<dyn std::error::Error>> {
    let shell = detect_shell()?;

    match shell.as_str() {
        "bash" => install_bash()?,
        "zsh" => install_zsh()?,
        "fish" => install_fish()?,
        "powershell" | "pwsh" => install_powershell()?,
        _ => {
            return Err(format!(
                "Unsupported shell: {}. Supported: bash, zsh, fish, powershell",
                shell
            )
            .into());
        }
    }

    println!("  Shell completions installed for {}.", shell);
    println!("  Restart your shell or open a new terminal to use them.");
    Ok(())
}

fn detect_shell() -> Result<String, Box<dyn std::error::Error>> {
    #[cfg(windows)]
    {
        return Ok("powershell".to_string());
    }

    #[cfg(not(windows))]
    {
        let shell_path =
            std::env::var("SHELL").map_err(|_| "Could not detect shell: $SHELL is not set")?;
        let shell = std::path::Path::new(&shell_path)
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("Could not parse shell name from $SHELL")?
            .to_string();
        Ok(shell)
    }
}

fn generate_to_buffer(shell: clap_complete::Shell) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut cmd = Args::command();
    clap_complete::generate(shell, &mut cmd, "mdx", &mut buf);
    buf
}

fn home_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
    #[cfg(windows)]
    {
        std::env::var("USERPROFILE")
            .map(PathBuf::from)
            .map_err(|_| "Could not determine home directory".into())
    }
    #[cfg(not(windows))]
    {
        std::env::var("HOME")
            .map(PathBuf::from)
            .map_err(|_| "Could not determine home directory".into())
    }
}

fn install_bash() -> Result<(), Box<dyn std::error::Error>> {
    let home = home_dir()?;
    let dir = home.join(".local/share/bash-completion/completions");
    fs::create_dir_all(&dir)?;

    let completions = generate_to_buffer(clap_complete::Shell::Bash);
    fs::write(dir.join("mdx"), &completions)?;

    // Source line for .bashrc
    let line = r#"[ -f "${HOME}/.local/share/bash-completion/completions/mdx" ] && . "${HOME}/.local/share/bash-completion/completions/mdx""#;
    let rc = bash_rc(&home);
    add_line_if_missing(&rc, line)?;

    Ok(())
}

fn install_zsh() -> Result<(), Box<dyn std::error::Error>> {
    let home = home_dir()?;
    let dir = home.join(".local/share/zsh/site-functions");
    fs::create_dir_all(&dir)?;

    let completions = generate_to_buffer(clap_complete::Shell::Zsh);
    fs::write(dir.join("_mdx"), &completions)?;

    let zshrc = home.join(".zshrc");
    let fpath_line = format!("fpath=(\"{dir}\" $fpath)", dir = dir.display());
    add_line_if_missing(&zshrc, &fpath_line)?;

    // Add compinit if not already present
    let compinit_line = "autoload -Uz compinit && compinit";
    let contents = fs::read_to_string(&zshrc).unwrap_or_default();
    if !contents.contains("compinit") {
        add_line_if_missing(&zshrc, compinit_line)?;
    }

    Ok(())
}

fn install_fish() -> Result<(), Box<dyn std::error::Error>> {
    let home = home_dir()?;
    let dir = home.join(".config/fish/completions");
    fs::create_dir_all(&dir)?;

    let completions = generate_to_buffer(clap_complete::Shell::Fish);
    fs::write(dir.join("mdx.fish"), &completions)?;

    // Fish auto-loads from this directory, no config changes needed
    Ok(())
}

fn install_powershell() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(windows)]
    {
        let local_app_data =
            std::env::var("LOCALAPPDATA").map_err(|_| "Could not determine LOCALAPPDATA")?;
        let dir = PathBuf::from(&local_app_data).join("mdx/completions");
        fs::create_dir_all(&dir)?;

        let completions = generate_to_buffer(clap_complete::Shell::PowerShell);
        let script_path = dir.join("mdx.ps1");
        fs::write(&script_path, &completions)?;

        // Detect the PowerShell profile path dynamically.
        // Try pwsh (PS 7+) first, fall back to legacy Windows PowerShell 5.1.
        let profile = detect_ps_profile(&local_app_data);
        if let Some(parent) = profile.parent() {
            fs::create_dir_all(parent)?;
        }
        let line = format!(". \"{}\"", script_path.display());
        add_line_if_missing(&profile, &line)?;
    }

    #[cfg(not(windows))]
    {
        eprintln!("  PowerShell completions install is only supported on Windows.");
        eprintln!("  Use 'mdx completions powershell' to print completions to stdout.");
    }

    Ok(())
}

/// Detect the correct PowerShell profile path.
/// Tries `pwsh` (PS 7+) first, falls back to legacy Windows PowerShell 5.1.
#[cfg(windows)]
fn detect_ps_profile(local_app_data: &str) -> PathBuf {
    use std::process::Command;

    // Try PowerShell 7+ (pwsh) — it knows its own profile path
    if let Ok(output) = Command::new("pwsh")
        .args(["-NoProfile", "-Command", "echo $PROFILE"])
        .output()
    {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return PathBuf::from(path);
            }
        }
    }

    // Fall back to legacy Windows PowerShell 5.1 profile path
    PathBuf::from(local_app_data)
        .join("Microsoft/Windows/PowerShell/Microsoft.PowerShell_profile.ps1")
}

fn bash_rc(home: &Path) -> PathBuf {
    let bashrc = home.join(".bashrc");
    if bashrc.exists() {
        bashrc
    } else {
        home.join(".bash_profile")
    }
}

/// Append `line` to the file at `path` if it doesn't already contain it.
fn add_line_if_missing(path: &Path, line: &str) -> Result<(), Box<dyn std::error::Error>> {
    if !path.exists() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::File::create(path)?;
    }

    let contents = fs::read_to_string(path)?;
    if contents.contains(line) {
        return Ok(());
    }

    let mut file = fs::OpenOptions::new().append(true).open(path)?;
    // Ensure we start on a new line
    if !contents.is_empty() && !contents.ends_with('\n') {
        writeln!(file)?;
    }
    writeln!(file, "{}", line)?;

    Ok(())
}
