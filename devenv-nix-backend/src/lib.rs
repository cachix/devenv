use anyhow::{Context, Result};
use devenv_core::config::Config;
use nix_bindings_fetchers::FetchersSettings;
use nix_bindings_flake::{
    FlakeInput, FlakeInputs, FlakeReference, FlakeReferenceParseFlags, FlakeSettings, LockFile,
};
use std::path::Path;

// Export the NixBackend implementation
pub mod nix_backend;
pub use nix_backend::ProjectRoot;

use std::cell::RefCell;

// Thread-local storage to keep GC registration guards alive for the thread's lifetime
thread_local! {
    static GC_REGISTRATION: RefCell<Option<nix_bindings_expr::eval_state::ThreadRegistrationGuard>> = const { RefCell::new(None) };
}

/// Register the current thread with Boehm GC.
///
/// This must be called from any thread that will access Nix/GC-managed memory.
/// Tokio worker threads should call this via `on_thread_start` to ensure
/// the GC can properly scan their stacks during collection.
///
/// Without this, parallel GC marking can race with unregistered threads,
/// causing memory corruption and crashes.
///
/// The registration is kept alive in thread-local storage until the thread exits.
pub fn gc_register_current_thread() -> Result<()> {
    use nix_bindings_expr::eval_state::gc_register_my_thread;

    GC_REGISTRATION.with(|reg| {
        let mut guard = reg.borrow_mut();
        if guard.is_none() {
            match gc_register_my_thread() {
                Ok(registration) => {
                    *guard = Some(registration);
                    Ok(())
                }
                Err(e) => Err(anyhow::anyhow!("Failed to register thread with GC: {}", e)),
            }
        } else {
            // Already registered
            Ok(())
        }
    })
}

// Activity logger integration with tracing
pub mod logger;

// Extension trait for anyhow::Result conversion
pub mod anyhow_ext;

// Cachix daemon client for pushing store paths
pub mod cachix_daemon;

/// Convert devenv inputs to FlakeInputs
///
/// # Arguments
/// * `fetch_settings` - Fetcher configuration
/// * `flake_settings` - Flake configuration
/// * `config` - Devenv configuration
pub fn create_flake_inputs(
    fetch_settings: &FetchersSettings,
    flake_settings: &FlakeSettings,
    config: &Config,
) -> Result<FlakeInputs> {
    let mut flake_inputs = FlakeInputs::new()?;

    let mut parse_flags = FlakeReferenceParseFlags::new(flake_settings)?;
    // Preserve relative paths so they can be resolved during locking via source_path context
    parse_flags.set_preserve_relative_paths(true)?;

    // Convert each devenv input to a FlakeInput
    for (name, input) in config.inputs.iter() {
        // Skip inputs without a URL (e.g., those with only "follows")
        let url = match &input.url {
            Some(url) => url,
            None => continue,
        };

        let (flake_ref, _fragment) =
            FlakeReference::parse_with_fragment(fetch_settings, flake_settings, &parse_flags, url)?;

        let mut flake_input = FlakeInput::new(&flake_ref, input.flake)?;

        // Set follows relationship before adding to collection
        // (C API does not support modifying inputs after they're added)
        if let Some(follows_path) = &input.follows {
            flake_input.set_follows(follows_path)?;
        }

        // Handle nested input overrides (e.g., git-hooks.inputs.nixpkgs.follows = "nixpkgs")
        if !input.inputs.is_empty() {
            let mut overrides = FlakeInputs::new()?;

            for (nested_name, nested_input) in input.inputs.iter() {
                let mut nested_flake_input = if let Some(nested_url) = &nested_input.url {
                    // Nested input has a URL - parse it
                    let (nested_ref, _) = FlakeReference::parse_with_fragment(
                        fetch_settings,
                        flake_settings,
                        &parse_flags,
                        nested_url,
                    )?;
                    FlakeInput::new(&nested_ref, nested_input.flake)?
                } else if let Some(follows_target) = &nested_input.follows {
                    // Nested input has only follows - use follows target as placeholder reference
                    // (the reference gets cleared internally by set_follows)
                    let (placeholder_ref, _) = FlakeReference::parse_with_fragment(
                        fetch_settings,
                        flake_settings,
                        &parse_flags,
                        follows_target,
                    )?;
                    FlakeInput::new(&placeholder_ref, true)?
                } else {
                    // Nested input has neither URL nor follows - skip it
                    continue;
                };

                // Set follows if specified
                if let Some(follows_path) = &nested_input.follows {
                    nested_flake_input.set_follows(follows_path)?;
                }

                overrides.add(nested_name, nested_flake_input)?;
            }

            // Set the overrides on the parent input
            flake_input.set_overrides(overrides)?;
        }

        flake_inputs.add(name, flake_input)?;
    }

    // Add nixpkgs as a default input if not already specified
    if !config.inputs.contains_key("nixpkgs") {
        let nixpkgs_url = "github:cachix/devenv-nixpkgs/rolling";
        let (flake_ref, _fragment) = FlakeReference::parse_with_fragment(
            fetch_settings,
            flake_settings,
            &parse_flags,
            nixpkgs_url,
        )?;
        let flake_input = FlakeInput::new(&flake_ref, true)?;
        flake_inputs.add("nixpkgs", flake_input)?;
    }

    // Add devenv as a default input if not already specified
    if !config.inputs.contains_key("devenv") {
        let devenv_url = "github:cachix/devenv?dir=src/modules";
        let (flake_ref, _fragment) = FlakeReference::parse_with_fragment(
            fetch_settings,
            flake_settings,
            &parse_flags,
            devenv_url,
        )?;
        let flake_input = FlakeInput::new(&flake_ref, true)?;
        flake_inputs.add("devenv", flake_input)?;
    }

    Ok(flake_inputs)
}

/// Load an existing lock file if it exists
pub fn load_lock_file(
    fetch_settings: &FetchersSettings,
    lock_path: &Path,
) -> Result<Option<LockFile>> {
    if lock_path.exists() {
        let content = std::fs::read_to_string(lock_path)?;
        let lock_path_str = lock_path
            .to_str()
            .context("Lock file path contains invalid UTF-8")?;
        let lock = LockFile::parse(fetch_settings, &content, Some(lock_path_str))?;
        Ok(Some(lock))
    } else {
        Ok(None)
    }
}

/// Write a lock file to disk
pub fn write_lock_file(lock_file: &LockFile, output_path: &Path) -> Result<()> {
    let lock_json = lock_file.to_string()?;
    std::fs::write(output_path, lock_json)
        .with_context(|| format!("Failed to write lock file to {}", output_path.display()))?;
    Ok(())
}
