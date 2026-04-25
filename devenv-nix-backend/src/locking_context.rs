//! Pre-bootstrap context for lock-file work.
//!
//! Today's bootstrap nix expression depends on `devenv.lock` to materialize
//! module inputs, so the framework must validate the lock and compute its
//! fingerprint before the assembled backend exists. `LockingContext` owns
//! just the FFI primitives needed for that work: store, fetchers/flake
//! settings, and an eval state.
//!
//! When the backend takes over the lock-file lifecycle, this type goes away.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use devenv_activity::instrument_activity;
use devenv_core::config::Input;
use devenv_core::{DevenvPaths, NixSettings};
use miette::{Result, WrapErr};
use nix_bindings_expr::eval_state::{
    EvalState, EvalStateBuilder, ThreadRegistrationGuard, gc_register_my_thread,
};
use nix_bindings_fetchers::FetchersSettings;
use nix_bindings_flake::{EvalStateBuilderExt, FlakeSettings};
use nix_bindings_store::store::Store;
use nix_bindings_util::settings;

use crate::anyhow_ext::AnyhowToMiette;
use crate::nix_backend::NixCBackend;

pub struct LockingContext {
    root: PathBuf,
    store: Store,
    eval_state: Mutex<EvalState>,
    flake_settings: FlakeSettings,
    fetchers_settings: FetchersSettings,
    _gc_registration: ThreadRegistrationGuard,
}

// SAFETY: the inner FFI types are wrapped in Mutex / used single-threaded
// and `LockingContext` is short-lived. Mirrors the Send/Sync contract on
// `NixCBackend` (see `nix_backend.rs` for the full safety analysis).
unsafe impl Send for LockingContext {}
unsafe impl Sync for LockingContext {}

impl LockingContext {
    /// Construct a context for lock-file work.
    ///
    /// `cachix_global_settings` is the result of
    /// `CachixManager::get_global_settings()` (e.g. a `netrc-file` path) and
    /// must be applied to the Nix settings registry before opening the store
    /// so authenticated fetches use the correct credentials.
    pub fn new(
        paths: &DevenvPaths,
        nix_settings: &NixSettings,
        cachix_global_settings: &std::collections::HashMap<String, String>,
    ) -> Result<Self> {
        crate::nix_init();

        let gc_registration = gc_register_my_thread()
            .to_miette()
            .wrap_err("Failed to register thread with Nix garbage collector")?;

        settings::set("experimental-features", "flakes nix-command")
            .to_miette()
            .wrap_err("Failed to enable experimental features")?;

        NixCBackend::apply_nix_settings(nix_settings)?;

        for (key, value) in cachix_global_settings {
            settings::set(key, value)
                .to_miette()
                .wrap_err_with(|| format!("Failed to set cachix global setting: {key}"))?;
        }

        let store = Store::open(None, [])
            .to_miette()
            .wrap_err("Failed to open Nix store")?;

        let flake_settings = FlakeSettings::new()
            .to_miette()
            .wrap_err("Failed to create flake settings")?;

        let fetchers_settings = FetchersSettings::new()
            .to_miette()
            .wrap_err("Failed to create fetchers settings")?;

        let root = paths.root.clone();
        let root_str = root
            .to_str()
            .ok_or_else(|| miette::miette!("Root path contains invalid UTF-8"))?;

        let eval_state = EvalStateBuilder::new(store.clone())
            .to_miette()
            .wrap_err("Failed to create eval state builder")?
            .base_directory(root_str)
            .to_miette()
            .wrap_err("Failed to set base directory")?
            .flakes(&flake_settings)
            .to_miette()
            .wrap_err("Failed to configure flakes in eval state")?
            .build()
            .to_miette()
            .wrap_err("Failed to build eval state")?;

        Ok(Self {
            root,
            store,
            eval_state: Mutex::new(eval_state),
            flake_settings,
            fetchers_settings,
            _gc_registration: gc_registration,
        })
    }

    /// Validate (and create or update if needed) `<root>/devenv.lock`.
    #[instrument_activity("Validating lock", kind = evaluate, level = DEBUG)]
    pub fn validate_lock_file(&self, inputs: &BTreeMap<String, Input>) -> Result<()> {
        let eval_state = self
            .eval_state
            .lock()
            .map_err(|_| miette::miette!("LockingContext eval state mutex poisoned"))?;
        crate::validate_lock_file(
            &eval_state,
            &self.fetchers_settings,
            &self.flake_settings,
            &self.root,
            inputs,
        )
        .to_miette()
    }

    /// Hash the locked inputs into a fingerprint used as the eval-cache seed.
    #[instrument_activity("Computing lock fingerprint", kind = evaluate, level = DEBUG)]
    pub fn lock_fingerprint(&self) -> Result<String> {
        let lock_file_path = self.root.join("devenv.lock");
        let lock_file = crate::load_lock_file(&self.fetchers_settings, &lock_file_path)
            .to_miette()
            .wrap_err("Failed to load lock file for fingerprint computation")?;
        crate::compute_lock_fingerprint(lock_file.as_ref(), &self.fetchers_settings, &self.store)
            .to_miette()
            .wrap_err("Failed to compute lock fingerprint")
    }
}
