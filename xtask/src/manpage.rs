use clap::CommandFactory;
use devenv::cli::Cli;
use miette::{IntoDiagnostic, Result};
use std::fs;
use std::path::{Path, PathBuf};

pub fn generate(out_dir: impl AsRef<Path>) -> Result<()> {
    fs::create_dir_all(&out_dir).into_diagnostic()?;
    clap_mangen::generate_to(Cli::command(), &out_dir).into_diagnostic()?;
    eprintln!("Generated man pages to {}", out_dir.as_ref().display());
    Ok(())
}

pub fn default_out_dir() -> PathBuf {
    std::env::current_dir().unwrap().join("man")
}
