use clap::CommandFactory;
use devenv::cli::Cli;
use miette::{IntoDiagnostic, Result};
use std::fs;
use std::path::{Path, PathBuf};

pub fn generate(shell: clap_complete::Shell, out_dir: impl AsRef<Path>) -> Result<()> {
    fs::create_dir_all(&out_dir).into_diagnostic()?;
    let mut cmd = Cli::command();
    let bin_name = cmd.get_name().to_string();
    let completion_path = clap_complete::generate_to(shell, &mut cmd, bin_name, out_dir.as_ref())
        .into_diagnostic()?;
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
