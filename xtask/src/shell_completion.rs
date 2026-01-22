use miette::{IntoDiagnostic, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn generate(shell: clap_complete::Shell, out_dir: impl AsRef<Path>) -> Result<()> {
    fs::create_dir_all(&out_dir).into_diagnostic()?;

    let shell_name = match shell {
        clap_complete::Shell::Bash => "bash",
        clap_complete::Shell::Zsh => "zsh",
        clap_complete::Shell::Fish => "fish",
        clap_complete::Shell::PowerShell => "powershell",
        clap_complete::Shell::Elvish => "elvish",
        _ => bail!("Unsupported shell"),
    };

    // Find devenv binary - first check next to current exe (local cargo build),
    // then fall back to PATH (nix build where $out/bin is in PATH)
    let devenv_bin = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("devenv")))
        .filter(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from("devenv"));

    // Generate dynamic completions by calling devenv with COMPLETE env var
    let output = Command::new(&devenv_bin)
        .env("COMPLETE", shell_name)
        .output()
        .into_diagnostic()?;

    if !output.status.success() {
        bail!(
            "Failed to generate completions: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let extension = match shell {
        clap_complete::Shell::Bash => "bash",
        clap_complete::Shell::Zsh => "zsh",
        clap_complete::Shell::Fish => "fish",
        clap_complete::Shell::PowerShell => "ps1",
        clap_complete::Shell::Elvish => "elv",
        _ => "txt",
    };

    // zsh completions should be named _devenv
    let filename = if shell == clap_complete::Shell::Zsh {
        "_devenv".to_string()
    } else {
        format!("devenv.{}", extension)
    };

    let completion_path = out_dir.as_ref().join(&filename);
    fs::write(&completion_path, &output.stdout).into_diagnostic()?;

    eprintln!(
        "Generated {} completions to {}",
        shell,
        completion_path.display()
    );
    Ok(())
}

pub fn default_out_dir() -> PathBuf {
    std::env::current_dir().unwrap()
}
