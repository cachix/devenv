use anyhow::{Context, Result};
use devenv_core::config::Config;
use nix_bindings_fetchers::FetchersSettings;
use nix_bindings_flake::{
    FlakeInput, FlakeInputs, FlakeReference, FlakeReferenceParseFlags, FlakeSettings, LockFile,
};
use nix_bindings_store::store::Store;
use std::path::Path;
use std::sync::Once;

// Export the NixBackend implementation
pub mod nix_backend;
pub use nix_backend::ProjectRoot;

use std::cell::RefCell;

// Ensure Nix/GC is initialized exactly once across all threads
static NIX_INIT: Once = Once::new();

// Thread-local storage to keep GC registration guards alive for the thread's lifetime
thread_local! {
    static GC_REGISTRATION: RefCell<Option<nix_bindings_expr::eval_state::ThreadRegistrationGuard>> = const { RefCell::new(None) };
}

/// Initialize the Nix expression library and Boehm GC.
///
/// This is safe to call multiple times - initialization only happens once.
/// Must be called before any thread tries to register with GC.
pub fn nix_init() {
    NIX_INIT.call_once(|| {
        nix_bindings_expr::eval_state::init().expect("Failed to initialize Nix expression library");
        install_gc_sp_corrector();
    });
}

/// Install a Boehm GC stack pointer corrector for Linux.
///
/// Nix uses boost coroutines internally (in `sourceToSink`/`sinkToSource` for
/// store I/O). When a thread is inside a coroutine, its stack pointer (sp)
/// points to the coroutine's fiber stack, not the original thread stack.
///
/// During GC collection, Boehm scans each thread's stack from sp to its
/// registered stack base. When sp is on a coroutine stack, this range is
/// wrong — the GC either scans nothing or scans an unrelated memory region,
/// missing roots on the real thread stack. This causes premature collection
/// of live objects, leading to heap corruption (SIGABRT in free/munmap_chunk).
///
/// The corrector detects when sp is outside the thread's OS stack and adjusts
/// it to the stack bottom, so the entire thread stack is scanned.
///
/// Nix had this fix but reverted it (commit 46b690734) because the macOS
/// implementation caused segfaults. The Linux implementation was fine, so
/// we install it here for Linux only.
fn install_gc_sp_corrector() {
    #[cfg(target_os = "linux")]
    {
        unsafe {
            nix_bindings_bindgen_raw::GC_set_sp_corrector(Some(fixup_boehm_stack_pointer));
        }
    }
}

/// Adapted from Nix's `fixupBoehmStackPointer` in `src/libexpr/eval-gc.cc`
/// (added in c4d903ddb, removed in ca0f7db84, restored in 3ba103865, reverted in 46b690734).
/// Still necessary because Nix uses boost coroutines for store I/O
/// (`sourceToSink`/`sinkToSource` in `serialise.cc`) and their fiber stacks
/// are not registered with the GC.
#[cfg(target_os = "linux")]
unsafe extern "C" fn fixup_boehm_stack_pointer(
    sp_ptr: *mut *mut std::os::raw::c_void,
    pthread_id: *mut std::os::raw::c_void,
) {
    use std::os::raw::c_void;

    // pthread types and functions from glibc — linked transitively through Nix.
    unsafe extern "C" {
        fn pthread_getattr_np(
            thread: libc_pthread_t,
            attr: *mut PthreadAttr,
        ) -> std::os::raw::c_int;
        fn pthread_attr_getstack(
            attr: *const PthreadAttr,
            stackaddr: *mut *mut c_void,
            stacksize: *mut usize,
        ) -> std::os::raw::c_int;
        fn pthread_attr_destroy(attr: *mut PthreadAttr) -> std::os::raw::c_int;
    }

    // pthread_t is c_ulong on Linux.
    type libc_pthread_t = std::os::raw::c_ulong;

    // pthread_attr_t is 56 bytes on x86_64, 64 on aarch64. Use 64 to cover both.
    #[repr(C)]
    struct PthreadAttr {
        _data: [u8; 64],
    }

    let sp = *sp_ptr;
    let thread = pthread_id as libc_pthread_t;

    let mut attr = std::mem::MaybeUninit::<PthreadAttr>::zeroed().assume_init();

    if pthread_getattr_np(thread, &mut attr) != 0 {
        return;
    }

    let mut stack_addr: *mut c_void = std::ptr::null_mut();
    let mut stack_size: usize = 0;

    if pthread_attr_getstack(&attr, &mut stack_addr, &mut stack_size) != 0 {
        pthread_attr_destroy(&mut attr);
        return;
    }

    pthread_attr_destroy(&mut attr);

    // stack_addr is the lowest address; stack grows down from stack_addr + stack_size.
    let stack_base = (stack_addr as *const u8).add(stack_size) as *mut c_void;

    if sp >= stack_base || sp < stack_addr {
        // sp is outside the OS thread stack (inside a coroutine).
        // Push sp to the bottom so the GC scans the entire thread stack.
        *sp_ptr = stack_addr;
    }
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
        let mut flake_input = if let Some(url) = &input.url {
            let (flake_ref, _fragment) =
                FlakeReference::parse_with_fragment(fetch_settings, flake_settings, &parse_flags, url)?;
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

/// Write a lock file to disk, only if content changed (to preserve mtime for direnv)
pub fn write_lock_file(lock_file: &LockFile, output_path: &Path) -> Result<()> {
    let lock_json = lock_file.to_string()?;
    // Compare with existing content to avoid updating mtime unnecessarily.
    // direnv watches devenv.lock and uses mtime to detect changes.
    if let Ok(existing) = std::fs::read_to_string(output_path) {
        if existing == lock_json {
            return Ok(());
        }
    }
    std::fs::write(output_path, &lock_json)
        .with_context(|| format!("Failed to write lock file to {}", output_path.display()))?;
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
pub fn compute_lock_fingerprint(lock_file: Option<&LockFile>, store: &Store) -> Result<String> {
    let mut parts: Vec<String> = Vec::new();

    if let Some(lock) = lock_file {
        let mut iter = lock.inputs_iterator()?;

        // The iterator starts pointing at the first element
        loop {
            let attr_path = iter.attr_path()?;
            if let Some(fingerprint) = iter.fingerprint(store)? {
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
    let hash = blake3::hash(combined.as_bytes());
    Ok(hash.to_hex().to_string())
}
