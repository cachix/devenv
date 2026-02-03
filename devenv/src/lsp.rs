//! LSP (Language Server Protocol) support for devenv.nix files
//!
//! This module provides integration with nixd to enable IDE features like
//! autocomplete, hover documentation, and go-to-definition for devenv.nix files.

use miette::{IntoDiagnostic, Result};
use serde::Serialize;
use std::collections::HashMap;
use std::os::unix::process::CommandExt;
use std::process::Command;
use tracing::info;

use crate::Devenv;

/// nixd configuration structure
#[derive(Serialize)]
struct NixdConfig {
    nixd: NixdSettings,
}

#[derive(Serialize)]
struct NixdSettings {
    nixpkgs: NixpkgsConfig,
    options: HashMap<String, OptionEntry>,
}

#[derive(Serialize)]
struct NixpkgsConfig {
    expr: String,
}

#[derive(Serialize)]
struct OptionEntry {
    expr: String,
}

/// Run the LSP server
pub async fn run(devenv: &Devenv, print_config: bool) -> Result<()> {
    // Assemble and get the serialized NixArgs
    let nix_args = devenv.assemble(false).await?;

    let bootstrap_path = devenv.dotfile().join("bootstrap");

    // Expression that imports default.nix with proper args and accesses .project.options
    let options_expr = format!(
        "(import {}/default.nix {}).project.options",
        bootstrap_path.display(),
        nix_args
    );

    // Expression for nixpkgs - same import but access .pkgs
    let nixpkgs_expr = format!(
        "(import {}/default.nix {}).pkgs",
        bootstrap_path.display(),
        nix_args
    );

    let mut options = HashMap::new();
    options.insert("devenv".to_string(), OptionEntry { expr: options_expr });

    let config = NixdConfig {
        nixd: NixdSettings {
            nixpkgs: NixpkgsConfig { expr: nixpkgs_expr },
            options,
        },
    };

    let config_json = serde_json::to_string_pretty(&config).into_diagnostic()?;

    if print_config {
        println!("{}", config_json);
        return Ok(());
    }

    // Find nixd (should be bundled with devenv)
    let nixd = which::which("nixd").into_diagnostic().map_err(|_| {
        miette::miette!("nixd not found in PATH. Ensure devenv is properly installed.")
    })?;

    // Launch nixd with the configuration
    // nixd accepts --config for initial configuration as a JSON string
    let config_str = serde_json::to_string(&config).into_diagnostic()?;

    info!(
        devenv.is_user_message = true,
        "Starting nixd language server"
    );

    // Use exec() to replace the current process with nixd
    // This ensures proper stdio handling for LSP communication
    // Set the working directory to the devenv root for proper path resolution
    let error = Command::new(&nixd)
        .arg("--config")
        .arg(&config_str)
        .current_dir(devenv.root())
        .exec();

    // exec() only returns if there was an error
    Err(miette::miette!("Failed to exec nixd: {}", error))
}
