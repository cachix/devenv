use anyhow::{Context, Result};
use devenv_core::config::Input;
use nix_bindings_expr::eval_state::EvalState;
use nix_bindings_fetchers::FetchersSettings;
use nix_bindings_flake::{
    FlakeInput, FlakeInputs, FlakeReference, FlakeReferenceParseFlags, FlakeSettings, InputsLocker,
    LockFile, LockMode,
};
use nix_bindings_store::store::Store;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Once;

pub mod backend;
pub use backend::{NixCBackend, ProjectRoot};

pub mod cnix_store;
pub use cnix_store::CNixStore;

pub mod lock;

use std::cell::RefCell;

// Ensure Nix/GC is initialized exactly once across all threads
static NIX_INIT: Once = Once::new();

// Thread-local storage to keep GC registration guards alive for the thread's lifetime
thread_local! {
    static GC_REGISTRATION: RefCell<Option<nix_bindings_expr::eval_state::ThreadRegistrationGuard>> = const { RefCell::new(None) };
}

/// Trigger the Nix interrupt flag to abort any in-progress Nix evaluation.
///
/// This sets a process-global flag that the Nix evaluator checks periodically.
/// When set, the evaluator throws an error and aborts the current operation.
///
/// Safe to call even when no Nix operation is running — the flag is simply set
/// and will be checked when the next evaluation starts.
pub fn trigger_interrupt() {
    nix_bindings_util::trigger_interrupt();
}

/// Initialize the Nix expression library and Boehm GC.
///
/// This is safe to call multiple times - initialization only happens once.
/// Must be called before any thread tries to register with GC.
pub fn nix_init() {
    NIX_INIT.call_once(|| {
        // Suppress Boehm GC "Repeated allocation of very large block" warnings.
        // These are harmless and would otherwise be printed directly to stderr,
        // bypassing our activity logger.
        if std::env::var_os("GC_LARGE_ALLOC_WARN_INTERVAL").is_none() {
            // SAFETY: Called once during single-threaded initialization (inside Once::call_once)
            // before any worker threads are spawned.
            unsafe { std::env::set_var("GC_LARGE_ALLOC_WARN_INTERVAL", "1000000") };
        }
        nix_bindings_expr::eval_state::init().expect("Failed to initialize Nix expression library");
    });
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
///
/// Note: This function ensures Nix/GC is initialized before registering,
/// so it's safe to call from any thread at any time.
pub fn gc_register_current_thread() -> Result<()> {
    use nix_bindings_expr::eval_state::gc_register_my_thread;

    // Ensure GC is initialized before any thread tries to register.
    // Without this, registering threads before GC_INIT() causes
    // signal handlers to not be properly installed, leading to
    // "Signals delivery fails" errors during stop-the-world collection.
    nix_init();

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

// Pure Rust BuildEnvironment parsing (for cached -env JSON)
pub mod build_environment;

// Cachix daemon client for pushing store paths
pub mod cachix_daemon;

// Wire protocol types for the cachix daemon socket
pub mod cachix_protocol;

// Scoped umask guard for Nix C API calls
pub mod umask_guard;

// Helpers for shaping Nix errors into miette diagnostics
mod error;

/// Convert devenv inputs to FlakeInputs
///
/// # Arguments
/// * `fetch_settings` - Fetcher configuration
/// * `flake_settings` - Flake configuration
/// * `inputs` - Input specifications from devenv.yaml
pub fn create_flake_inputs(
    fetch_settings: &FetchersSettings,
    flake_settings: &FlakeSettings,
    inputs: &BTreeMap<String, Input>,
) -> Result<FlakeInputs> {
    let mut flake_inputs = FlakeInputs::new()?;

    let mut parse_flags = FlakeReferenceParseFlags::new(flake_settings)?;
    // Preserve relative paths so they can be resolved during locking via source_path context
    parse_flags.set_preserve_relative_paths(true)?;

    // Convert each devenv input to a FlakeInput
    for (name, input) in inputs.iter() {
        let mut flake_input = if let Some(url) = &input.url {
            let (flake_ref, _fragment) = FlakeReference::parse_with_fragment(
                fetch_settings,
                flake_settings,
                &parse_flags,
                url,
            )?;
            FlakeInput::new(&flake_ref, input.flake)?
        } else if let Some(follows_target) = &input.follows {
            // Top-level input has only follows - use follows target as placeholder reference
            // (the reference gets cleared internally by set_follows)
            let (placeholder_ref, _) = FlakeReference::parse_with_fragment(
                fetch_settings,
                flake_settings,
                &parse_flags,
                follows_target,
            )?;
            FlakeInput::new(&placeholder_ref, true)?
        } else {
            continue;
        };

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
    if !inputs.contains_key("nixpkgs") {
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
    if !inputs.contains_key("devenv") {
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

/// Write a lock file to disk, only if content changed (to preserve mtime for direnv)
pub fn write_lock_file(lock_file: &LockFile, output_path: &Path) -> Result<()> {
    let lock_json = lock_file.to_string()?;
    // Compare with existing content to avoid updating mtime unnecessarily.
    // direnv watches devenv.lock and uses mtime to detect changes.
    if let Ok(existing) = std::fs::read_to_string(output_path)
        && existing == lock_json
    {
        return Ok(());
    }
    std::fs::write(output_path, &lock_json)
        .with_context(|| format!("Failed to write lock file to {}", output_path.display()))?;
    Ok(())
}

/// Lock the requested inputs against an existing lock file (if any) and write
/// the result to `<root>/devenv.lock`.
///
/// `input_name = Some(name)` updates a single input; `None` updates all.
/// `override_inputs` is a flat list of `[name, url, name, url, ...]` parsed
/// in pairs.
///
/// The caller owns the [`EvalState`] lock; this function makes one synchronous
/// FFI call into the locker and writes the lock file on success.
pub fn lock_inputs(
    eval_state: &EvalState,
    fetch_settings: &FetchersSettings,
    flake_settings: &FlakeSettings,
    root: &Path,
    inputs: &BTreeMap<String, Input>,
    input_name: Option<&str>,
    override_inputs: &[String],
) -> Result<()> {
    let flake_inputs = create_flake_inputs(fetch_settings, flake_settings, inputs)
        .context("Failed to create flake inputs")?;

    let lock_file_path = root.join("devenv.lock");
    let old_lock =
        load_lock_file(fetch_settings, &lock_file_path).context("Failed to load lock file")?;

    let base_dir_str = root.to_str().context("Root path contains invalid UTF-8")?;
    // Nix's resolveRelativePath uses parent() on the source_path to get the directory.
    // Since it expects a file path (like flake.nix), we append devenv.nix so parent() returns the root.
    let source_path = root.join("devenv.nix");
    let source_path_str = source_path
        .to_str()
        .context("Source path contains invalid UTF-8")?;

    let mut locker = InputsLocker::new(flake_settings)
        .with_inputs(flake_inputs)
        .source_path(source_path_str)
        .mode(LockMode::Virtual)
        .use_registries(true);

    if let Some(lock) = &old_lock {
        locker = locker.old_lock_file(lock);
    }

    if let Some(name) = input_name {
        locker = locker.update_input(name);
    } else {
        locker = locker.update_all();
    }

    let overrides: Vec<(String, FlakeReference)> = if !override_inputs.is_empty() {
        let mut parse_flags = FlakeReferenceParseFlags::new(flake_settings)?;
        parse_flags.set_base_directory(base_dir_str)?;

        override_inputs
            .chunks_exact(2)
            .map(|pair| {
                let (override_ref, _) = FlakeReference::parse_with_fragment(
                    fetch_settings,
                    flake_settings,
                    &parse_flags,
                    &pair[1],
                )?;
                Ok((pair[0].clone(), override_ref))
            })
            .collect::<Result<Vec<_>>>()
            .context("Failed to parse input overrides")?
    } else {
        Vec::new()
    };

    if !overrides.is_empty() {
        locker = locker.overrides(overrides.iter().map(|(name, r)| (name.clone(), r)));
    }

    let lock_file = {
        let _guard = crate::umask_guard::UmaskGuard::restrictive();
        locker
            .lock(fetch_settings, eval_state)
            .context("Failed to lock inputs")?
    };

    write_lock_file(&lock_file, &lock_file_path).context("Failed to write lock file")?;

    Ok(())
}

/// Validate the existing lock file against the current inputs, regenerating it
/// if missing, unparseable, or out of date.
///
/// On success, `<root>/devenv.lock` exists and is consistent with `inputs`.
pub fn validate_lock_file(
    eval_state: &EvalState,
    fetch_settings: &FetchersSettings,
    flake_settings: &FlakeSettings,
    root: &Path,
    inputs: &BTreeMap<String, Input>,
) -> Result<()> {
    let lock_file_path = root.join("devenv.lock");

    if !lock_file_path.exists() {
        return lock_inputs(
            eval_state,
            fetch_settings,
            flake_settings,
            root,
            inputs,
            None,
            &[],
        );
    }

    let old_lock =
        load_lock_file(fetch_settings, &lock_file_path).context("Failed to load lock file")?;
    let Some(old_lock) = old_lock else {
        return lock_inputs(
            eval_state,
            fetch_settings,
            flake_settings,
            root,
            inputs,
            None,
            &[],
        );
    };

    let flake_inputs = create_flake_inputs(fetch_settings, flake_settings, inputs)
        .context("Failed to create flake inputs")?;

    let source_path = root.join("devenv.nix");
    let source_path_str = source_path
        .to_str()
        .context("Source path contains invalid UTF-8")?;

    // Virtual mode so unlocked local inputs don't fail validation;
    // we compare the computed lock against the existing one to detect drift.
    let locker = InputsLocker::new(flake_settings)
        .with_inputs(flake_inputs)
        .source_path(source_path_str)
        .old_lock_file(&old_lock)
        .mode(LockMode::Virtual)
        .use_registries(true);

    let lock_result = {
        let _guard = crate::umask_guard::UmaskGuard::restrictive();
        locker.lock(fetch_settings, eval_state)
    };

    let new_lock = lock_result.context("Lock validation failed")?;
    if new_lock.has_changes(&old_lock)? {
        tracing::debug!("Lock validation found changes, writing updated lock");
        // Writing new_lock directly avoids re-fetching every input: it was
        // computed with old_lock as a base, so unchanged inputs are preserved.
        write_lock_file(&new_lock, &lock_file_path).context("Failed to write lock file")?;
    }
    Ok(())
}

/// Compute a content fingerprint from all locked inputs' fingerprints.
///
/// This iterates over all locked nodes and combines their fingerprints into
/// a single hash. The fingerprint for each input varies by type:
/// - git/github/mercurial: the revision hash
/// - tarball/path: the narHash in SRI format
///
/// Returns a hex-encoded BLAKE3 hash of the combined fingerprints.
/// If no lock file exists or no inputs have fingerprints, returns the hash of an empty string.
pub fn compute_lock_fingerprint(
    lock_file: Option<&LockFile>,
    fetch_settings: &FetchersSettings,
    store: &Store,
) -> Result<String> {
    let mut parts: Vec<String> = Vec::new();

    if let Some(lock) = lock_file {
        let mut iter = lock.inputs_iterator()?;

        // The iterator starts pointing at the first element
        loop {
            let attr_path = iter.attr_path()?;
            if let Some(fingerprint) = iter.fingerprint(fetch_settings, store)? {
                tracing::debug!("attr_path: {}, fingerprint: {}", attr_path, fingerprint);
                parts.push(format!("{}={}", attr_path, fingerprint));
            }
            if !iter.next() {
                break;
            }
        }
    }

    // Sort for deterministic output
    parts.sort();

    let combined = parts.join(";");
    Ok(devenv_cache_core::compute_string_hash(&combined))
}
