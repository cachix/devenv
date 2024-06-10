use clap::CommandFactory;
use miette::{IntoDiagnostic, Result};
use std::fs;
use std::path::{Path, PathBuf};

mod cli {
    include!("../../devenv/src/cli.rs");
}

pub fn generate_manpages(out_dir: impl AsRef<Path>) -> Result<()> {
    fs::create_dir_all(&out_dir).into_diagnostic()?;
    clap_mangen::generate_to(cli::Cli::command(), &out_dir).into_diagnostic()?;
    println!("Generated man pages to {}", out_dir.as_ref().display());
    Ok(())
}

pub fn default_out_dir() -> PathBuf {
    std::env::current_dir().unwrap().join("man")
}
