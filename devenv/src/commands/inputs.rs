//! `devenv inputs`: manage inputs in `devenv.yaml`.

use devenv_core::config::Config;
use miette::{IntoDiagnostic, Result};

pub fn add(name: &str, url: &str, follows: &[String]) -> Result<()> {
    let config = Config::load()?;
    let root_dir = std::env::current_dir().into_diagnostic()?;
    if !config.add_input_to_file(&root_dir, name, url, follows)? {
        eprintln!(
            "devenv: rewrote devenv.yaml without its comments; its `inputs` are not a block mapping \
             this command can edit in place"
        );
    }
    Ok(())
}
