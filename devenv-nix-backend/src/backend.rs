//! C-Nix backend (`NixCBackend`).
//!
//! Owns the FFI primitives — store, eval state, settings, activity logger,
//! GC registration — plus devenv's intra-process eval caching, the cached
//! root devenv `Value`, and the devenv-specific primop bindings.
//!
//! Cachix push, port-allocator allocation, and shutdown coordination live
//! on `Devenv`, not here.
//!
//! Construction is two-phase, with phase 1 spelled out at the call site
//! rather than hidden behind an aggregator helper. The pattern is:
//!
//! ```ignore
//! let _gc = init_nix(&nix_settings, &store_settings)?;
//! let store = open_store(&store_settings)?;
//! let (flake_settings, fetchers_settings) = build_settings()?;
//! let logger_setup = logger::setup_nix_logger()?;
//! let fingerprint = lock::with_lock_scope(&logger_setup.bridge, || {
//!     let eval_state = lock::build_eval_state(&store, &root, &flake_settings)?;
//!     lock::validate_and_load(&eval_state, &store, &fetchers_settings,
//!         &flake_settings, &root, &lock_file, &inputs)
//! })?;
//! let bootstrap_args = build_bootstrap_args(..., &fingerprint)?;
//! let backend = NixCBackend::new(
//!     paths, nix_settings, cache_settings, nixpkgs_config,
//!     store, flake_settings, fetchers_settings, _gc,
//!     bootstrap_args, primops, eval_context, eval_cache_pool, logger_setup,
//! )?;
//! ```

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use async_trait::async_trait;
use devenv_activity::{Activity, ActivityInstrument, activity, instrument_activity};
use devenv_cache_core::compute_string_hash;
use devenv_core::bootstrap_args::BootstrapArgs;
use devenv_core::cachix::{NetrcPreservation, preserve_netrc_file};
use devenv_core::config::NixpkgsConfig;
use devenv_core::evaluator::eval_cache_key_args;
use devenv_core::evaluator::{
    BuildOptions, DevEnvOutput, Evaluator, PackageSearchResult, SearchResults,
};
use devenv_core::nix_args::NixpkgsConfigForNix;
use devenv_core::nix_log_bridge::{EvalActivityGuard, NixLogBridge};
use devenv_core::realized::RealizedPathsObserver;
use devenv_core::store::StorePath as CoreStorePath;
use devenv_core::store::{GcOptions, Store as StoreTrait};
use devenv_core::{CacheSettings, DevenvPaths, NixSettings, StoreSettings};
use devenv_eval_cache::{
    self, CachedEval, CachingConfig, CachingEvalService, CachingEvalState, EvalCacheKey,
    EvalContext,
};
use miette::{IntoDiagnostic, Result, WrapErr, bail, miette};
use nix_bindings_expr::eval_state::{
    EvalState, EvalStateBuilder, ThreadRegistrationGuard, gc_register_my_thread,
};
use nix_bindings_expr::to_json::value_to_json;
use nix_bindings_expr::{EvalCache, SearchParams, SearchResult, search};
use nix_bindings_fetchers::FetchersSettings;
use nix_bindings_flake::{EvalStateBuilderExt, FlakeSettings};
use nix_bindings_store::build_env::BuildEnvironment;
use nix_bindings_store::store::{Store, TrustedFlag};
use nix_bindings_util::settings;
use nix_cmd::ReplExitStatus;
use once_cell::sync::OnceCell;
use tracing::Instrument;

use crate::anyhow_ext::AnyhowToMiette;
use crate::build_environment::BuildEnvironment as RustBuildEnvironment;
use crate::cnix_store::CNixStore;
use crate::error::format_eval_error;
use crate::primops::PrimopRegistry;
use crate::umask_guard::UmaskGuard;

/// Initialize Nix FFI globals, register the calling thread with the GC,
/// and apply process-global Nix settings (experimental features, options
/// from `nix_settings`, and `netrc-file` from `store_settings`).
///
/// The returned guard must be kept alive for as long as the calling
/// thread holds Nix/GC state — typically, hand it to
/// [`NixCBackend::new`], which adopts ownership.
pub fn init_nix(
    nix_settings: &NixSettings,
    store_settings: &StoreSettings,
) -> Result<ThreadRegistrationGuard> {
    crate::nix_init();
    let gc_registration = gc_register_my_thread()
        .to_miette()
        .wrap_err("Failed to register thread with Nix garbage collector")?;
    settings::set("extra-experimental-features", "flakes nix-command")
        .to_miette()
        .wrap_err("Failed to enable experimental features")?;
    apply_nix_settings(nix_settings)?;
    // Best effort, as in `apply_store_settings`: without netrc, private
    // substituters fall back to unauthenticated requests, which beats
    // refusing to start at all.
    if let Err(e) = apply_netrc_setting(store_settings) {
        tracing::warn!("Failed to set netrc-file: {}", e);
    }
    Ok(gc_registration)
}

/// Open the Nix store and apply substituters / trusted public keys from
/// `store_settings`. Must be called after [`init_nix`].
pub fn open_store(store_settings: &StoreSettings) -> Result<Store> {
    let store = Store::open(None, [])
        .to_miette()
        .wrap_err("Failed to open Nix store")?;
    apply_substituters_and_keys(&store, store_settings);
    Ok(store)
}

/// Build the flake + fetchers settings used by both the lock helpers and
/// the long-lived backend `EvalState`.
pub fn build_settings() -> Result<(FlakeSettings, FetchersSettings)> {
    let flake_settings = FlakeSettings::new()
        .to_miette()
        .wrap_err("Failed to create flake settings")?;
    let fetchers_settings = FetchersSettings::new()
        .to_miette()
        .wrap_err("Failed to create fetchers settings")?;
    Ok((flake_settings, fetchers_settings))
}

/// Specifies where the project root is located.
#[derive(Debug, Clone)]
pub enum ProjectRoot {
    Path(PathBuf),
    InputRef(String),
}

impl Default for ProjectRoot {
    fn default() -> Self {
        ProjectRoot::Path(PathBuf::from("."))
    }
}

/// FFI-based Nix backend implementation.
///
/// Field declaration order matters for `Drop`: FFI fields drop bottom-up
/// in the order `caching_eval_state → eval_state → store → settings →
/// activity_logger → _gc_registration` so C++ destructors run with their
/// dependencies still alive.
pub struct NixCBackend {
    pub nix_settings: NixSettings,
    pub cache_settings: CacheSettings,
    pub paths: DevenvPaths,

    bootstrap_path: PathBuf,
    nixpkgs_config_path: PathBuf,

    nix_log_bridge: Arc<NixLogBridge>,
    eval_cache_pool: Option<Arc<tokio::sync::OnceCell<sqlx::SqlitePool>>>,

    bootstrap_args: Arc<BootstrapArgs>,
    primops: PrimopRegistry,

    cached_devenv_value: Mutex<Option<nix_bindings_expr::value::Value>>,
    devenv_value_invalidated: Arc<AtomicBool>,
    caching_eval_state: OnceCell<CachingEvalState<Arc<Mutex<Option<EvalState>>>>>,

    eval_state: Arc<Mutex<Option<EvalState>>>,

    cnix_store: CNixStore,

    #[allow(dead_code)]
    flake_settings: FlakeSettings,
    pub(crate) fetchers_settings: FetchersSettings,

    activity_logger: nix_bindings_expr::logger::ActivityLogger,

    /// Observers notified per-realization in `build`/`dev_env`, gated on
    /// `!cache_hit`. Registered at startup; calls are sync and must not
    /// block (see [`RealizedPathsObserver`]).
    realized_observers: Mutex<Vec<Arc<dyn RealizedPathsObserver>>>,

    #[allow(dead_code)]
    _gc_registration: ThreadRegistrationGuard,
}

// SAFETY: concurrent access to FFI types is gated by the Mutex on
// `eval_state`; the rest are immutable after construction or use C-side
// locking.
unsafe impl Send for NixCBackend {}
unsafe impl Sync for NixCBackend {}

fn core_config_watch_paths(root: &Path, lock_file: &Path) -> Vec<PathBuf> {
    [
        "devenv.nix",
        "devenv.yaml",
        "devenv.local.nix",
        "devenv.local.yaml",
    ]
    .into_iter()
    .map(|path| root.join(path))
    .chain(std::iter::once(lock_file.to_path_buf()))
    .filter(|path| path.exists())
    .collect()
}

fn eval_cache_error_into_miette(e: devenv_eval_cache::Error<miette::Error>) -> miette::Error {
    match e {
        devenv_eval_cache::Error::Eval(err) => err,
        // Preserve the source chain (sqlx/io/serde_json) instead of stringifying.
        devenv_eval_cache::Error::Internal(c) => Err::<(), _>(c).into_diagnostic().unwrap_err(),
    }
}

/// Build the logical `<storedir>/<basename>` store-path string from a path that
/// may have been `real_path`-translated for a relocated/chroot store.
///
/// `Store::parse_store_path` only accepts the logical form (e.g.
/// `/nix/store/<hash>-<name>`), but several call sites cache the *real* path
/// returned by [`Store::real_path`], which differs for a relocated store
/// (e.g. `/srv/nix/store/<hash>-<name>`). The basename is identical between the
/// two forms, so the logical path is just the store's logical dir joined with
/// that basename. Returns `None` if the path has no basename. See devenv #2499.
fn logical_store_path_str(storedir: &str, path: &str) -> Option<String> {
    let basename = std::path::Path::new(path).file_name()?.to_str()?;
    Some(format!("{}/{}", storedir.trim_end_matches('/'), basename))
}

/// Parse a possibly `real_path`-translated store path into its logical
/// [`StorePath`]. Use instead of `store.parse_store_path` whenever the input may
/// be a cached real path (gc-root creation). See [`logical_store_path_str`].
fn parse_logical_store_path(
    store: &mut Store,
    path: &str,
) -> Result<nix_bindings_store::path::StorePath> {
    let storedir = store
        .get_storedir()
        .to_miette()
        .wrap_err("Failed to get store directory")?;
    let logical = logical_store_path_str(&storedir, path)
        .ok_or_else(|| miette!("store path '{}' has no basename", path))?;
    store
        .parse_store_path(&logical)
        .to_miette()
        .wrap_err("Failed to parse store path")
}

fn cache_key_for(
    bootstrap_args: &BootstrapArgs,
    primops: &PrimopRegistry,
    attr_name: &str,
) -> EvalCacheKey {
    let cache_key_args =
        eval_cache_key_args(bootstrap_args.as_str(), &primops.cache_key_fragment());
    EvalCacheKey::from_nix_args_str(&cache_key_args, attr_name)
}

/// RAII guard that holds the eval-state lock and registers an activity
/// for file evaluations during the session.
pub(crate) struct EvalSession<'a> {
    guard: std::sync::MutexGuard<'a, Option<EvalState>>,
    _eval_activity: EvalActivityGuard<'a>,
}

impl std::ops::Deref for EvalSession<'_> {
    type Target = EvalState;
    fn deref(&self) -> &Self::Target {
        self.guard.as_ref().expect("EvalState not available")
    }
}

impl std::ops::DerefMut for EvalSession<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.guard.as_mut().expect("EvalState not available")
    }
}

impl NixCBackend {
    /// Build a long-lived backend on top of the FFI primitives produced
    /// by phase 1 of construction (see module docs). Builds the
    /// long-lived `EvalState`, sets up the activity logger, wraps the
    /// store, wires intra-process caching, and adopts the
    /// `gc_registration` guard for the backend's lifetime.
    #[allow(clippy::too_many_arguments)]
    #[instrument_activity("Initializing Nix backend", kind = operation, level = TRACE)]
    pub fn new(
        paths: DevenvPaths,
        nix_settings: NixSettings,
        cache_settings: CacheSettings,
        nixpkgs_config: &NixpkgsConfig,
        store: Store,
        flake_settings: FlakeSettings,
        fetchers_settings: FetchersSettings,
        gc_registration: ThreadRegistrationGuard,
        bootstrap_args: Arc<BootstrapArgs>,
        primops: PrimopRegistry,
        eval_context: EvalContext,
        eval_cache_pool: Option<Arc<tokio::sync::OnceCell<sqlx::SqlitePool>>>,
        logger_setup: crate::logger::NixLoggerSetup,
    ) -> Result<Self> {
        let bootstrap_path = extract_bootstrap_files(&paths.dotfile)?;
        let nixpkgs_config_path = write_nixpkgs_config(nixpkgs_config, &paths.dotfile)?;

        // Scope the lazy `«nix-internal»` load to the surrounding activity.
        let eval_state = {
            let current_span = tracing::Span::current();
            let _eval_guard = devenv_activity::current_activity_id()
                .map(|id| logger_setup.bridge.begin_eval_with_span(id, current_span));
            build_eval_state(
                &store,
                &paths.root,
                &nixpkgs_config_path,
                &flake_settings,
                nix_settings.nix_debugger,
                nix_settings.refresh_fetchers,
            )?
        };

        let activity_logger = logger_setup.logger;
        let nix_log_bridge = logger_setup.bridge;

        let cnix_store = CNixStore::new(store);
        // `EvalState` is neither `Send` nor `Sync`, but `NixCBackend` asserts
        // both (see the `unsafe impl`s above) and shares this handle across
        // threads, so the refcount has to stay atomic. `Rc` would race.
        #[allow(clippy::arc_with_non_send_sync)]
        let eval_state = Arc::new(Mutex::new(Some(eval_state)));

        let backend = Self {
            nix_settings,
            cache_settings,
            paths,
            bootstrap_path,
            nixpkgs_config_path,
            nix_log_bridge,
            eval_cache_pool,
            bootstrap_args,
            primops,
            cached_devenv_value: Mutex::new(None),
            devenv_value_invalidated: Arc::new(AtomicBool::new(false)),
            caching_eval_state: OnceCell::new(),
            eval_state,
            cnix_store,
            flake_settings,
            fetchers_settings,
            activity_logger,
            realized_observers: Mutex::new(Vec::new()),
            _gc_registration: gc_registration,
        };
        backend.init_caching_eval_state(eval_context);
        Ok(backend)
    }

    fn init_caching_eval_state(&self, eval_context: EvalContext) {
        if self.caching_eval_state.get().is_some() {
            return;
        }
        let cache_key_args = eval_cache_key_args(
            self.bootstrap_args.as_str(),
            &self.primops.cache_key_fragment(),
        );

        let cached_eval = if let Some(pool_cell) = &self.eval_cache_pool {
            if let Some(pool) = pool_cell.get() {
                let config = CachingConfig {
                    force_refresh: self.cache_settings.refresh_eval_cache,
                    extra_watch_paths: core_config_watch_paths(
                        &self.paths.root,
                        &self.paths.lock_file,
                    ),
                    excluded_envs: vec!["NIXPKGS_CONFIG".to_string()],
                    excluded_paths: vec![self.nixpkgs_config_path.clone()],
                };
                let service = CachingEvalService::with_config(pool.clone(), config.clone());
                let invalidation_flag = self.devenv_value_invalidated.clone();
                CachedEval::with_cache_and_inputs(
                    service,
                    self.nix_log_bridge.clone(),
                    config,
                    eval_context.inputs().clone(),
                )
                .with_on_cached_state_invalidation(Arc::new(move || {
                    invalidation_flag.store(true, Ordering::Release);
                }))
            } else {
                CachedEval::without_cache_and_inputs(
                    self.nix_log_bridge.clone(),
                    eval_context.inputs().clone(),
                )
            }
        } else {
            CachedEval::without_cache_and_inputs(
                self.nix_log_bridge.clone(),
                eval_context.inputs().clone(),
            )
        }
        .with_resources(eval_context.resources().clone());

        let caching_eval_state =
            CachingEvalState::new(self.eval_state.clone(), cached_eval, cache_key_args);
        let _ = self.caching_eval_state.set(caching_eval_state);
    }

    pub fn cache_key(&self, attr_name: &str) -> EvalCacheKey {
        cache_key_for(&self.bootstrap_args, &self.primops, attr_name)
    }

    pub fn paths(&self) -> &DevenvPaths {
        &self.paths
    }

    pub fn fetchers_settings(&self) -> &FetchersSettings {
        &self.fetchers_settings
    }

    pub fn flake_settings(&self) -> &FlakeSettings {
        &self.flake_settings
    }

    pub fn store_handle(&self) -> &Store {
        self.cnix_store.inner()
    }

    pub fn eval_state_handle(&self) -> &Arc<Mutex<Option<EvalState>>> {
        &self.eval_state
    }

    /// Build a fresh transient `EvalState` against the same store and
    /// settings. Used by lock helpers; the caller drops it when done.
    pub fn fresh_eval_state(&self) -> Result<EvalState> {
        build_eval_state(
            self.cnix_store.inner(),
            &self.paths.root,
            &self.nixpkgs_config_path,
            &self.flake_settings,
            self.nix_settings.nix_debugger,
            self.nix_settings.refresh_fetchers,
        )
    }

    pub fn bootstrap_file(&self, relative_path: &str) -> PathBuf {
        self.bootstrap_path.join(relative_path)
    }

    fn eval_session(&self, activity: &Activity) -> Result<EvalSession<'_>> {
        let guard = self
            .eval_state
            .lock()
            .map_err(|e| miette!("Failed to lock eval state: {}", e))?;
        if guard.is_none() {
            bail!("EvalState is not available (hot-reload may have failed to create a new one)");
        }
        let eval_activity = self
            .nix_log_bridge
            .begin_eval_with_span(activity.id(), activity.span());
        Ok(EvalSession {
            guard,
            _eval_activity: eval_activity,
        })
    }

    fn eval_import_with_primops(
        &self,
        eval_state: &mut EvalState,
    ) -> Result<nix_bindings_expr::value::Value> {
        let args_nix = self.bootstrap_args.as_str();
        let base = self.paths.root.to_str().unwrap();

        let import_path = self.bootstrap_file("default.nix");
        let import_nix_path = ser_nix::to_string(&ser_nix::NixPathBuf::from(import_path))
            .into_diagnostic()
            .wrap_err("Failed to serialize import path")?;
        let import_fn = eval_state
            .eval_from_string(&format!("import ({import_nix_path})"), base)
            .to_miette()
            .wrap_err("Failed to evaluate import expression")?;

        let base_args = eval_state
            .eval_from_string(args_nix, base)
            .to_miette()
            .wrap_err("Failed to evaluate bootstrap args")?;

        let primops_attrset = self.primops.register_all(eval_state)?;

        let override_attrs = eval_state
            .new_value_attrs(vec![("primops".to_string(), primops_attrset)])
            .to_miette()
            .wrap_err("Failed to create override attrset")?;

        let merge_fn = eval_state
            .eval_from_string("a: b: a // b", "<primop-injection>")
            .to_miette()
            .wrap_err("Failed to create merge function")?;
        let final_args = eval_state
            .call_multi(&merge_fn, &[base_args, override_attrs])
            .to_miette()
            .wrap_err("Failed to merge args with primops")?;

        eval_state
            .call(import_fn, final_args)
            .to_miette()
            .wrap_err("Failed to evaluate devenv configuration")
    }

    fn get_or_eval_devenv(
        &self,
        eval_state: &mut EvalState,
    ) -> Result<nix_bindings_expr::value::Value> {
        if self.devenv_value_invalidated.swap(false, Ordering::AcqRel) {
            let mut cached = self
                .cached_devenv_value
                .lock()
                .map_err(|e| miette!("Failed to lock cached devenv value: {}", e))?;
            *cached = None;
        }
        {
            let cached = self
                .cached_devenv_value
                .lock()
                .map_err(|e| miette!("Failed to lock cached devenv value: {}", e))?;
            if let Some(value) = cached.as_ref() {
                return Ok(value.clone());
            }
        }
        let value = self.eval_import_with_primops(eval_state)?;
        let returned = value.clone();
        let mut cached = self
            .cached_devenv_value
            .lock()
            .map_err(|e| miette!("Failed to lock cached devenv value: {}", e))?;
        *cached = Some(value);
        Ok(returned)
    }

    fn enriched<T>(&self, result: anyhow::Result<T>, context: impl AsRef<str>) -> Result<T> {
        result
            .to_miette()
            .map_err(|e| self.enrich_eval_error(e, context.as_ref()))
    }

    fn enrich_eval_error(&self, err: miette::Error, context: &str) -> miette::Error {
        // Always render the eval-returned error (`{err:#}` flattens the FFI
        // cause chain), never a log line captured during evaluation — a Nix
        // warning forwarded at error verbosity could otherwise shadow the real
        // error. See `error::format_eval_error` for the rendering rules.
        miette::Report::from(format_eval_error(&format!("{err:#}"), context))
    }

    fn eval_attr_uncached(
        &self,
        attr_path: &str,
        clean_path: &str,
        activity: &Activity,
    ) -> Result<String> {
        let mut eval_state = self.eval_session(activity)?;
        let root_attrs = self.get_or_eval_devenv(&mut eval_state)?;

        let value = self.enriched(
            eval_state.require_attrs_select(&root_attrs, clean_path),
            format!("Failed to get attribute '{}'", attr_path),
        )?;

        self.enriched(
            eval_state.force(&value),
            format!("Failed to force evaluation of '{}'", attr_path),
        )?;

        let json_value = match value_to_json(&mut eval_state, &value) {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(error = %e, "Failed to convert {} to JSON", attr_path);
                return Err(miette!("Failed to convert {} to JSON: {}", attr_path, e));
            }
        };

        serde_json::to_string(&json_value)
            .into_diagnostic()
            .wrap_err(format!("Failed to serialize {} to JSON", attr_path))
    }

    fn build_shell_uncached(&self, activity: &Activity) -> Result<CachedShellPaths> {
        macro_rules! shell_stage {
            ($name:literal, $kind:literal, $stage:literal, $body:expr $(, $($field:ident).+ = $value:expr)*) => {{
                let span = tracing::debug_span!(
                    target: "devenv_activity::spans",
                    "shell_stage",
                    otel.name = $name,
                    devenv.activity.kind = $kind,
                    devenv.shell.stage = $stage,
                    devenv.outcome = tracing::field::Empty,
                    otel.status_code = tracing::field::Empty
                    $(, $($field).+ = $value)*
                );
                let _nix_callback_parent =
                    self.nix_log_bridge.enter_eval_tracing_span(&span);
                let result = span.in_scope(|| $body);
                if result.is_err() {
                    span.record("devenv.outcome", "failed");
                    span.record("otel.status_code", "ERROR");
                } else {
                    span.record("devenv.outcome", "success");
                }
                result
            }};
        }

        let mut eval_state = shell_stage!(
            "opening shell evaluation session",
            "operation",
            "open_eval_session",
            self.eval_session(activity)
        )?;

        let devenv = shell_stage!(
            "evaluating devenv configuration",
            "evaluate",
            "evaluate_configuration",
            self.get_or_eval_devenv(&mut eval_state)
        )?;

        let shell_drv = shell_stage!(
            "selecting shell derivation",
            "evaluate",
            "select_derivation",
            self.enriched(
                eval_state.require_attrs_select(&devenv, "shell"),
                "Failed to get shell attribute",
            )
        )?;

        shell_stage!(
            "forcing shell derivation",
            "evaluate",
            "force_derivation",
            self.enriched(
                eval_state.force(&shell_drv),
                "Failed to force evaluation of shell derivation",
            )
        )?;

        let drv_path_value = shell_stage!(
            "selecting shell drvPath",
            "evaluate",
            "select_drv_path",
            self.enriched(
                eval_state.require_attrs_select(&shell_drv, "drvPath"),
                "Failed to get drvPath from shell derivation",
            )
        )?;

        let drv_path = shell_stage!(
            "extracting shell drvPath",
            "evaluate",
            "extract_drv_path",
            self.enriched(
                eval_state.require_string(&drv_path_value),
                "Failed to extract drvPath as string",
            )
        )?;

        let out_path_value = shell_stage!(
            "selecting shell outPath",
            "evaluate",
            "select_out_path",
            self.enriched(
                eval_state.require_attrs_select(&shell_drv, "outPath"),
                "Failed to get outPath from shell derivation",
            )
        )?;

        let realized = shell_stage!(
            "realizing shell derivation",
            "build",
            "realize_derivation",
            {
                let _guard = UmaskGuard::restrictive();
                self.enriched(
                    eval_state.realise_string(&out_path_value, false),
                    "Failed to realize shell derivation",
                )
            },
            devenv.derivation_path = drv_path.as_str()
        )?;

        let (mut store, out_path) = shell_stage!(
            "resolving realized shell path",
            "operation",
            "resolve_output_path",
            {
                let store_path = realized
                    .paths
                    .first()
                    .ok_or_else(|| miette!("Shell derivation produced no output paths"))?;
                let mut store = self.cnix_store.inner().clone();
                let out_path = store
                    .real_path(store_path)
                    .to_miette()
                    .wrap_err("Failed to get store path")?;
                Ok::<_, miette::Report>((store, out_path))
            }
        )?;

        let env_path = shell_stage!(
            "extracting shell environment",
            "operation",
            "extract_environment",
            {
                let drv_store_path = store
                    .parse_store_path(&drv_path)
                    .to_miette()
                    .wrap_err("Failed to parse derivation store path")?;
                let (_build_env, env_store_path) = {
                    let _guard = UmaskGuard::restrictive();
                    BuildEnvironment::get_dev_environment(self.cnix_store.inner(), &drv_store_path)
                        .to_miette()
                        .wrap_err("Failed to get dev environment")?
                };
                let env_path = store
                    .real_path(&env_store_path)
                    .to_miette()
                    .wrap_err("Failed to get env store path")?;
                Ok::<_, miette::Report>(Some(env_path))
            }
        )?;

        Ok(CachedShellPaths {
            drv_path,
            out_path,
            env_path,
        })
    }

    fn build_attr_uncached(&self, attr_path: &str, activity: &Activity) -> Result<String> {
        let mut eval_state = self.eval_session(activity)?;
        let root_attrs = self.get_or_eval_devenv(&mut eval_state)?;

        let value = self.enriched(
            eval_state.require_attrs_select(&root_attrs, attr_path),
            format!("Failed to get attribute '{}'", attr_path),
        )?;
        self.enriched(
            eval_state.force(&value),
            format!("Failed to evaluate attribute: {}", attr_path),
        )?;

        let build_value = self
            .enriched(
                eval_state.require_attrs_select_opt(&value, "outPath"),
                format!("Failed to check for outPath in attribute: {}", attr_path),
            )?
            .unwrap_or_else(|| value.clone());

        let realized = {
            let _guard = UmaskGuard::restrictive();
            self.enriched(
                eval_state.realise_string(&build_value, false),
                format!("Failed to build attribute: {}", attr_path),
            )?
        };

        let store_path = realized
            .paths
            .first()
            .ok_or_else(|| miette!("Attribute '{}' produced no output paths", attr_path))?;

        let mut store = self.cnix_store.inner().clone();
        let path_str = store
            .real_path(store_path)
            .to_miette()
            .wrap_err("Failed to get store path")?;

        Ok(path_str)
    }

    fn build_dev_environment(
        &self,
        store: &mut Store,
        drv_path_str: &str,
        json: bool,
    ) -> Result<String> {
        let drv_store_path = store
            .parse_store_path(drv_path_str)
            .to_miette()
            .wrap_err("Failed to parse derivation store path")?;

        let (mut build_env, _env_path) =
            BuildEnvironment::get_dev_environment(self.cnix_store.inner(), &drv_store_path)
                .to_miette()
                .wrap_err("Failed to get dev environment from derivation")?;

        if json {
            build_env
                .to_json()
                .to_miette()
                .wrap_err("Failed to serialize environment to JSON")
        } else {
            let env_json = build_env
                .to_json()
                .to_miette()
                .wrap_err("Failed to serialize environment to JSON")?;
            let rust_env = RustBuildEnvironment::from_json(&env_json)
                .into_diagnostic()
                .wrap_err("Failed to parse environment JSON")?;
            Ok(rust_env.to_activation_script())
        }
    }

    /// Evaluate the dev shell, producing the bash env script (or JSON).
    pub async fn dev_env(&self, json: bool, gc_root: &Path) -> Result<DevEnvOutput> {
        let output_format = if json { "json" } else { "activation_script" };
        let activity = devenv_activity::start!(
            Activity::evaluate("Evaluating shell"),
            devenv.shell.stage = "construct",
            devenv.shell.output_format = output_format,
            devenv.shell.environment_source = tracing::field::Empty,
            devenv.shell.environment_bytes = tracing::field::Empty,
            devenv.shell.input_count = tracing::field::Empty,
            devenv.cache.hit = tracing::field::Empty,
            devenv.cache.lookup_result = tracing::field::Empty
        );

        let result = self
            .dev_env_inner(json, gc_root, &activity)
            .in_activity(&activity)
            .await;

        match result {
            Ok((output, cache_hit)) => {
                activity.record("devenv.cache.hit", cache_hit);
                if cache_hit {
                    activity.cached();
                }
                Ok(output)
            }
            Err(error) => {
                activity.fail();
                Err(error)
            }
        }
    }

    async fn dev_env_inner(
        &self,
        json: bool,
        gc_root: &Path,
        activity: &Activity,
    ) -> Result<(DevEnvOutput, bool)> {
        let caching_state = self
            .caching_eval_state
            .get()
            .expect("caching eval state must be initialized");

        let cache_key = self.cache_key("shell");

        let cache_span = tracing::debug_span!(
            target: "devenv_activity::spans",
            "shell_cache_lookup",
            otel.name = "checking shell evaluation cache",
            devenv.activity.kind = "operation",
            devenv.shell.stage = "cache_lookup",
            devenv.cache.lookup_result = tracing::field::Empty,
            devenv.outcome = tracing::field::Empty
        );
        let (cached_paths, cache_lookup_result): (Option<CachedShellPaths>, &'static str) = async {
            if let Some(service) = caching_state.cached_eval().service() {
                match service.get_cached(&cache_key).await {
                    Ok(Some(cached)) => {
                        match serde_json::from_str::<CachedShellPaths>(&cached.json_output) {
                            Ok(paths) => {
                                let drv_exists = std::path::Path::new(&paths.drv_path).exists();
                                let out_exists = std::path::Path::new(&paths.out_path).exists();
                                if drv_exists && out_exists {
                                    match caching_state
                                        .cached_eval()
                                        .try_restore_cached_state(&cached)
                                        .await
                                    {
                                        Ok(()) => (Some(paths), "hit"),
                                        Err(e) => {
                                            tracing::warn!(error = %e, "Cached evaluation state restore failed for shell cache hit, re-evaluating");
                                            (None, "restore_failed")
                                        }
                                    }
                                } else {
                                    if let Err(db_err) = service.invalidate(&cache_key).await {
                                        tracing::warn!(error = %db_err, "Failed to invalidate stale shell cache entry");
                                    }
                                    (None, "stale_paths")
                                }
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "Failed to parse cached shell paths");
                                (None, "invalid_payload")
                            }
                        }
                    }
                    Ok(None) => (None, "miss"),
                    Err(e) => {
                        tracing::warn!(error = %e, "Error checking eval cache for shell");
                        (None, "lookup_failed")
                    }
                }
            } else {
                (None, "disabled")
            }
        }
        .instrument(cache_span.clone())
        .await;
        cache_span.record("devenv.cache.lookup_result", cache_lookup_result);
        activity.record("devenv.cache.lookup_result", cache_lookup_result);
        let cache_outcome = match cache_lookup_result {
            "hit" => "cached",
            "disabled" => "skipped",
            _ => "success",
        };
        cache_span.record("devenv.outcome", cache_outcome);

        let (drv_path_str, out_path_str, env_path, cache_hit) = if let Some(paths) = cached_paths {
            (paths.drv_path, paths.out_path, paths.env_path, true)
        } else {
            let result = caching_state
                .cached_eval()
                .eval_typed::<CachedShellPaths, _, _, _>(&cache_key, activity, || async {
                    self.build_shell_uncached(activity)
                })
                .await;

            match result {
                Ok((paths, cache_hit)) => {
                    (paths.drv_path, paths.out_path, paths.env_path, cache_hit)
                }
                Err(e) => return Err(eval_cache_error_into_miette(e)),
            }
        };

        crate::gc::collect(if cache_hit {
            "shell_cache_hit"
        } else {
            "shell_evaluation"
        });

        let mut store = self.cnix_store.inner().clone();
        // `out_path_str` is the real_path-translated path, which differs from the
        // logical store path under a relocated/chroot store; gc-root creation
        // needs the logical form. See devenv #2499.
        let store_path = parse_logical_store_path(&mut store, &out_path_str)?;
        let gc_root_span = tracing::debug_span!(
            target: "devenv_activity::spans",
            "shell_stage",
            otel.name = "registering shell gc root",
            devenv.activity.kind = "operation",
            devenv.shell.stage = "register_gc_root",
            devenv.outcome = tracing::field::Empty,
            otel.status_code = tracing::field::Empty
        );
        let gc_root_result: Result<Store> = gc_root_span.in_scope(|| {
            let mut store = self.cnix_store.inner().clone();
            // `out_path_str` is the real_path-translated path, which differs from the
            // logical store path under a relocated/chroot store; gc-root creation
            // needs the logical form. See devenv #2499.
            let store_path = parse_logical_store_path(&mut store, &out_path_str)?;

            if gc_root.symlink_metadata().is_ok() {
                std::fs::remove_file(gc_root)
                    .map_err(|e| miette!("Failed to remove existing GC root: {}", e))?;
            }
            store
                .add_perm_root(&store_path, gc_root)
                .to_miette()
                .wrap_err("Failed to create GC root")?;
            Ok(store)
        });
        if gc_root_result.is_err() {
            gc_root_span.record("devenv.outcome", "failed");
            gc_root_span.record("otel.status_code", "ERROR");
        } else {
            gc_root_span.record("devenv.outcome", "success");
        }
        let mut store = gc_root_result?;

        if !cache_hit {
            self.notify_realized(&[PathBuf::from(&out_path_str)]);
        }

        let environment_source = if env_path
            .as_deref()
            .is_some_and(|path| std::path::Path::new(path).exists())
        {
            "cache_file"
        } else if env_path.is_some() {
            "derivation_missing_cache_file"
        } else {
            "derivation"
        };
        activity.record("devenv.shell.environment_source", environment_source);
        let environment_span = tracing::debug_span!(
            target: "devenv_activity::spans",
            "shell_stage",
            otel.name = "loading shell environment",
            devenv.activity.kind = "operation",
            devenv.shell.stage = "load_environment",
            devenv.shell.environment_source = environment_source,
            devenv.shell.environment_bytes = tracing::field::Empty,
            devenv.outcome = tracing::field::Empty,
            otel.status_code = tracing::field::Empty
        );
        let output_result: Result<String> = environment_span.in_scope(|| {
            if let Some(env_path) = env_path.as_deref() {
                if std::path::Path::new(env_path).exists() {
                    let env_json = std::fs::read_to_string(env_path)
                        .into_diagnostic()
                        .wrap_err("Failed to read cached env JSON")?;
                    let rust_env = RustBuildEnvironment::from_json(&env_json)
                        .into_diagnostic()
                        .wrap_err("Failed to parse cached env JSON")?;

                    if json {
                        Ok(env_json)
                    } else {
                        Ok(rust_env.to_activation_script())
                    }
                } else {
                    self.build_dev_environment(&mut store, &drv_path_str, json)
                }
            } else {
                self.build_dev_environment(&mut store, &drv_path_str, json)
            }
        });
        if output_result.is_err() {
            environment_span.record("devenv.outcome", "failed");
            environment_span.record("otel.status_code", "ERROR");
        } else {
            environment_span.record("devenv.outcome", "success");
        }
        let output_str = output_result?;
        environment_span.record("devenv.shell.environment_bytes", output_str.len());
        activity.record("devenv.shell.environment_bytes", output_str.len());

        let inputs_span = tracing::debug_span!(
            target: "devenv_activity::spans",
            "shell_stage",
            otel.name = "loading shell evaluation inputs",
            devenv.activity.kind = "operation",
            devenv.shell.stage = "load_inputs",
            devenv.shell.input_count = tracing::field::Empty,
            devenv.outcome = tracing::field::Empty,
            otel.status_code = tracing::field::Empty
        );
        let (inputs, inputs_outcome) = if let Some(service) = caching_state.cached_eval().service()
        {
            match async { service.get_file_inputs(&cache_key).await }
                .instrument(inputs_span.clone())
                .await
            {
                Ok(inputs) => (inputs, "success"),
                Err(error) => {
                    inputs_span.record("otel.status_code", "ERROR");
                    tracing::warn!(%error, "Failed to load shell evaluation inputs");
                    (Vec::new(), "failed")
                }
            }
        } else {
            (Vec::new(), "skipped")
        };
        inputs_span.record("devenv.outcome", inputs_outcome);
        inputs_span.record("devenv.shell.input_count", inputs.len());
        activity.record("devenv.shell.input_count", inputs.len());

        Ok((
            DevEnvOutput {
                bash_env: output_str.into_bytes(),
                inputs,
            },
            cache_hit,
        ))
    }

    pub async fn prepare_repl(&self) -> Result<()> {
        nix_cmd::init()
            .to_miette()
            .wrap_err("Failed to initialize Nix command library")?;

        let activity = activity!(INFO, evaluate, "Evaluating Nix");
        let mut eval_state = self.eval_session(&activity)?;
        let devenv_attrs = self.get_or_eval_devenv(&mut eval_state)?;

        eval_state
            .force(&devenv_attrs)
            .to_miette()
            .wrap_err("Failed to evaluate devenv configuration")?;
        eval_state
            .require_attrs_select(&devenv_attrs, "pkgs")
            .to_miette()
            .wrap_err("Failed to evaluate pkgs attribute")?;

        Ok(())
    }

    pub async fn launch_repl(&self) -> Result<()> {
        self.activity_logger.reset();

        for error in self.nix_log_bridge.take_pre_repl_errors() {
            eprintln!("{}", error);
        }

        let activity = activity!(INFO, evaluate, "Launching REPL");
        let mut eval_state = self.eval_session(&activity)?;

        let status = if nix_cmd::debugger_is_pending() {
            nix_cmd::debugger_run_pending(&mut eval_state)
                .to_miette()
                .wrap_err("Debugger REPL failed")?
        } else {
            let devenv_attrs = self.get_or_eval_devenv(&mut eval_state)?;
            let mut env = nix_cmd::ValMap::new()
                .to_miette()
                .wrap_err("Failed to create REPL environment")?;
            env.insert("devenv", &devenv_attrs)
                .to_miette()
                .wrap_err("Failed to inject devenv into REPL scope")?;
            let pkgs = eval_state
                .require_attrs_select(&devenv_attrs, "pkgs")
                .to_miette()
                .wrap_err("Failed to get pkgs attribute")?;
            env.insert("pkgs", &pkgs)
                .to_miette()
                .wrap_err("Failed to inject pkgs into REPL scope")?;
            let inputs = eval_state
                .require_attrs_select(&devenv_attrs, "inputs")
                .to_miette()
                .wrap_err("Failed to get inputs attribute")?;
            env.insert("inputs", &inputs)
                .to_miette()
                .wrap_err("Failed to inject inputs into REPL scope")?;
            nix_cmd::run_repl_simple(&mut eval_state, Some(&mut env))
                .to_miette()
                .wrap_err("REPL failed")?
        };

        match status {
            ReplExitStatus::QuitAll => std::process::exit(0),
            ReplExitStatus::Continue => Ok(()),
        }
    }

    pub async fn search(&self, name: &str, max_results: Option<usize>) -> Result<SearchResults> {
        let activity = activity!(INFO, evaluate, "Searching packages");
        let mut eval_state = self.eval_session(&activity)?;

        let devenv = self.get_or_eval_devenv(&mut eval_state)?;
        let pkgs = self.enriched(
            eval_state.require_attrs_select(&devenv, "pkgs"),
            "Failed to get pkgs attribute",
        )?;

        let cache = EvalCache::new(&mut eval_state, &pkgs, None)
            .to_miette()
            .wrap_err("Failed to create eval cache for pkgs")?;
        let cursor = cache
            .root()
            .to_miette()
            .wrap_err("Failed to get root cursor from eval cache")?;

        let mut params = SearchParams::new()
            .to_miette()
            .wrap_err("Failed to create search params")?;
        params
            .add_regex(name)
            .to_miette()
            .wrap_err("Failed to add search regex")?;

        let mut results: SearchResults = Default::default();

        search(&cursor, Some(&params), |result: SearchResult| {
            if max_results.is_some_and(|max| results.len() >= max) {
                return false;
            }
            results.insert(
                result.attr_path,
                PackageSearchResult {
                    pname: result.name,
                    version: result.version,
                    description: result.description,
                },
            );
            true
        })
        .to_miette()
        .wrap_err("Search failed")?;

        Ok(results)
    }

    pub async fn gc(&self, paths: Vec<PathBuf>) -> Result<(u64, u64)> {
        let stats = self
            .cnix_store
            .collect_garbage(GcOptions {
                paths: Some(paths.into_iter().map(CoreStorePath::from).collect()),
                max_freed: 0,
            })
            .await?;
        Ok((stats.paths_deleted, stats.bytes_freed))
    }

    pub async fn is_trusted_user(&self) -> Result<bool> {
        let mut store = self.cnix_store.inner().clone();
        match store.is_trusted_client() {
            TrustedFlag::Trusted => Ok(true),
            TrustedFlag::NotTrusted => Ok(false),
            TrustedFlag::Unknown => Err(miette!(
                "Unable to determine trust status for Nix store (store type may not support trust queries)"
            )),
        }
    }

    /// Lock or update inputs. Convenience wrapper that uses a fresh
    /// transient `EvalState` and invalidates the long-lived one after.
    pub async fn update(
        &self,
        input_name: &Option<String>,
        inputs: &std::collections::BTreeMap<String, devenv_core::config::Input>,
        override_inputs: &[String],
    ) -> Result<()> {
        let eval_state = self.fresh_eval_state()?;
        let res = crate::lock::update(
            &eval_state,
            &self.fetchers_settings,
            &self.flake_settings,
            &self.paths.root,
            &self.paths.lock_file,
            inputs,
            input_name.as_deref(),
            override_inputs,
        );
        drop(eval_state);
        res?;
        self.invalidate_eval_state()
    }

    /// Evaluate a single attribute path against the user's devenv
    /// config root and return JSON, using a caller-supplied [`Activity`]
    /// for TUI/tracing instead of the generic "Evaluating Nix" label
    /// that [`Evaluator::eval`] emits. Use this when calling from
    /// internal-eval contexts (e.g., reading cachix config) where a
    /// descriptive label and DEBUG level are preferable.
    pub async fn eval_attr(&self, attr_path: &str, activity: &Activity) -> Result<String> {
        let caching_state = self
            .caching_eval_state
            .get()
            .expect("caching eval state must be initialized");

        let clean_path = attr_path.trim_start_matches(".#");
        let cache_key = self.cache_key(clean_path);
        let attr_path_owned = attr_path.to_string();
        let clean_path_owned = clean_path.to_string();

        let (json_str, _cache_hit) = async {
            caching_state
                .cached_eval()
                .eval(&cache_key, activity, || async {
                    self.eval_attr_uncached(&attr_path_owned, &clean_path_owned, activity)
                })
                .await
        }
        .in_activity(activity)
        .await
        .map_err(eval_cache_error_into_miette)?;

        Ok(json_str)
    }

    /// Apply substituters, trusted public keys, and the netrc-file path
    /// to the open store.
    ///
    /// Use after backend init when the cachix configuration has been
    /// evaluated. The `netrc-file` global must land before
    /// `add_substituter` runs — adding a substituter triggers an
    /// authenticated `nix-cache-info` probe, and a private cache without
    /// netrc would get 401. Failures are logged warn but never
    /// fatal — devenv continues without the cachix substituters.
    pub fn apply_store_settings(&self, store_settings: &StoreSettings) {
        // Open an eval scope on the bridge so substituter info fetches
        // (e.g. `nix-cache-info` downloads) fired from worker threads
        // inside the C call nest under the current TUI activity.
        let current_span = tracing::Span::current();
        let _eval_guard = devenv_activity::current_activity_id()
            .map(|id| self.nix_log_bridge.begin_eval_with_span(id, current_span));
        if let Err(e) = apply_netrc_setting(store_settings) {
            tracing::warn!("Failed to set netrc-file: {}", e);
        }
        apply_substituters_and_keys(self.cnix_store.inner(), store_settings);
    }

    /// Register an observer to be notified about freshly realized store
    /// paths. Observers are called inline on the evaluation thread, once
    /// per attribute build (and once for the shell derivation in
    /// `dev_env`), gated on `!cache_hit`. Implementations must be
    /// non-blocking.
    ///
    /// Typical use: a cachix push pump where the observer holds an
    /// `mpsc::UnboundedSender` and a separate task drains it into the
    /// daemon.
    pub fn add_realized_observer(&self, observer: Arc<dyn RealizedPathsObserver>) {
        if let Ok(mut guard) = self.realized_observers.lock() {
            guard.push(observer);
        }
    }

    fn notify_realized(&self, paths: &[PathBuf]) {
        if paths.is_empty() {
            return;
        }
        // Snapshot under the lock; release before invoking observers so
        // an observer that re-enters the backend cannot deadlock.
        let observers: Vec<Arc<dyn RealizedPathsObserver>> = match self.realized_observers.lock() {
            Ok(g) if g.is_empty() => return,
            Ok(g) => g.clone(),
            Err(_) => return,
        };
        for obs in &observers {
            obs.on_realized(paths);
        }
    }

    pub fn invalidate_eval_state(&self) -> Result<()> {
        self.cached_devenv_value
            .lock()
            .map_err(|e| miette!("Failed to clear cached devenv value: {e}"))?
            .take();

        if let Some(state) = self.caching_eval_state.get() {
            state.cached_eval().clear_eval_inputs();
        }

        let mut guard = self
            .eval_state
            .lock()
            .map_err(|e| miette!("Failed to lock eval state for replacement: {e}"))?;

        let old_state = guard.take();
        drop(old_state);

        let new_state = build_eval_state(
            self.cnix_store.inner(),
            &self.paths.root,
            &self.nixpkgs_config_path,
            &self.flake_settings,
            self.nix_settings.nix_debugger,
            self.nix_settings.refresh_fetchers,
        )?;
        *guard = Some(new_state);

        Ok(())
    }

    #[cfg(feature = "test-nix-store")]
    pub fn log_bridge(&self) -> &Arc<NixLogBridge> {
        &self.nix_log_bridge
    }

    #[cfg(feature = "test-nix-store")]
    pub fn eval_inputs(&self) -> Option<&Arc<devenv_eval_cache::EvalInputTracker>> {
        self.caching_eval_state
            .get()
            .map(|state| state.cached_eval().eval_inputs())
    }
}

#[async_trait(?Send)]
impl Evaluator for NixCBackend {
    fn name(&self) -> &str {
        "nix"
    }

    fn store(&self) -> &dyn StoreTrait {
        &self.cnix_store
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn eval(&self, attrs: &[&str]) -> Result<String> {
        let mut results = Vec::new();
        for attr_path in attrs {
            let activity = activity!(INFO, evaluate, "Evaluating Nix");
            let json_str = self.eval_attr(attr_path, &activity).await?;
            results.push(json_str);
        }

        if results.len() == 1 {
            Ok(results.into_iter().next().unwrap())
        } else {
            Ok(format!("[{}]", results.join(",")))
        }
    }

    async fn build(&self, attrs: &[&str], opts: BuildOptions) -> Result<Vec<CoreStorePath>> {
        if attrs.is_empty() {
            return Ok(Vec::new());
        }

        let caching_state = self
            .caching_eval_state
            .get()
            .expect("caching eval state must be initialized");

        let mut output_paths = Vec::new();

        for attr_path in attrs {
            let cache_key = self.cache_key(&format!("{}:build", attr_path));

            let cached_path: Option<String> = if let Some(service) =
                caching_state.cached_eval().service()
            {
                match service.get_cached(&cache_key).await {
                    Ok(Some(cached)) => match serde_json::from_str::<String>(&cached.json_output) {
                        Ok(path_str) => {
                            if std::path::Path::new(&path_str).exists() {
                                match caching_state
                                    .cached_eval()
                                    .try_restore_cached_state(&cached)
                                    .await
                                {
                                    Ok(()) => Some(path_str),
                                    Err(e) => {
                                        tracing::warn!(error = %e, attr_path = attr_path, "Cached evaluation state restore failed for build cache hit, re-evaluating");
                                        None
                                    }
                                }
                            } else {
                                if let Err(db_err) = service.invalidate(&cache_key).await {
                                    tracing::warn!(error = %db_err, attr_path = attr_path, "Failed to invalidate stale build cache entry");
                                }
                                None
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "Failed to parse cached build path");
                            None
                        }
                    },
                    Ok(None) => None,
                    Err(e) => {
                        tracing::warn!(error = %e, "Error checking build cache");
                        None
                    }
                }
            } else {
                None
            };

            let activity = activity!(INFO, evaluate, format!("Evaluating {}", attr_path));

            let cache_hit = cached_path.is_some();
            let path_str = if let Some(path) = cached_path {
                activity.cached();
                path
            } else {
                let attr_path_owned = attr_path.to_string();
                let (path, _) = async {
                    caching_state
                        .cached_eval()
                        .eval_typed::<String, _, _, _>(&cache_key, &activity, || async {
                            self.build_attr_uncached(&attr_path_owned, &activity)
                        })
                        .await
                }
                .in_activity(&activity)
                .await
                .map_err(eval_cache_error_into_miette)?;
                path
            };

            let path = PathBuf::from(&path_str);

            if !cache_hit {
                self.notify_realized(std::slice::from_ref(&path));
            }

            if let Some(gc_root_base) = &opts.gc_root {
                let mut store = self.cnix_store.inner().clone();
                // `path_str` is real_path-translated; gc-root creation needs the
                // logical store path (differs under a relocated store). See #2499.
                let store_path = parse_logical_store_path(&mut store, &path_str)?;

                let sanitized_attr = attr_path.replace('.', "-");
                let attr_gc_root = gc_root_base.with_file_name(format!(
                    "{}-{}",
                    gc_root_base
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy(),
                    sanitized_attr
                ));

                if attr_gc_root.symlink_metadata().is_ok() {
                    std::fs::remove_file(&attr_gc_root)
                        .map_err(|e| miette!("Failed to remove existing GC root: {}", e))?;
                }

                store
                    .add_perm_root(&store_path, &attr_gc_root)
                    .to_miette()
                    .wrap_err("Failed to add GC root")?;
            }

            output_paths.push(CoreStorePath::from(path));
        }

        Ok(output_paths)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CachedShellPaths {
    drv_path: String,
    out_path: String,
    #[serde(default)]
    env_path: Option<String>,
}

fn build_eval_state(
    store: &Store,
    root: &Path,
    nixpkgs_config_path: &Path,
    flake_settings: &FlakeSettings,
    enable_debugger: bool,
    refresh_fetchers: bool,
) -> Result<EvalState> {
    let root_str = root
        .to_str()
        .ok_or_else(|| miette!("Root path contains invalid UTF-8"))?;
    let nixpkgs_config_str = nixpkgs_config_path
        .to_str()
        .ok_or_else(|| miette!("Nixpkgs config path contains invalid UTF-8"))?;

    let mut builder = EvalStateBuilder::new(store.clone())
        .to_miette()
        .wrap_err("Failed to create eval state builder")?
        .base_directory(root_str)
        .to_miette()
        .wrap_err("Failed to set base directory")?
        .env_override("NIXPKGS_CONFIG", nixpkgs_config_str)
        .to_miette()
        .wrap_err("Failed to set NIXPKGS_CONFIG")?
        .flakes(flake_settings)
        .to_miette()
        .wrap_err("Failed to configure flakes")?;

    // `devenv update` sets this so branch and tag inputs are re-resolved
    // instead of served from cache within tarball-ttl. The eval state's own
    // fetchers::Settings is what governs locking, so the override must land
    // here rather than on the global config or the locker's settings argument.
    if refresh_fetchers {
        builder = builder
            .fetch_setting("tarball-ttl", "0")
            .to_miette()
            .wrap_err("Failed to set tarball-ttl")?;
    }

    let mut eval_state = builder
        .build()
        .to_miette()
        .wrap_err("Failed to build eval state")?;

    if enable_debugger {
        eval_state
            .enable_debugger()
            .to_miette()
            .wrap_err("Failed to enable debugger")?;
    }

    Ok(eval_state)
}

fn extract_bootstrap_files(dotfile_dir: &Path) -> Result<PathBuf> {
    use std::io::Write;

    static BOOTSTRAP_DIR: include_dir::Dir<'_> =
        include_dir::include_dir!("$CARGO_MANIFEST_DIR/bootstrap");

    let bootstrap_path = dotfile_dir.join("bootstrap");
    std::fs::create_dir_all(&bootstrap_path)
        .into_diagnostic()
        .wrap_err("Failed to create bootstrap directory")?;

    for file in BOOTSTRAP_DIR.files() {
        let target_path = bootstrap_path.join(file.path());
        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent)
                .into_diagnostic()
                .wrap_err("Failed to create parent directories")?;
        }
        if let Ok(existing) = std::fs::read(&target_path)
            && existing == file.contents()
        {
            continue;
        }
        let mut output_file = std::fs::File::create(&target_path)
            .into_diagnostic()
            .wrap_err(format!("Failed to create file: {}", target_path.display()))?;
        output_file
            .write_all(file.contents())
            .into_diagnostic()
            .wrap_err(format!("Failed to write file: {}", target_path.display()))?;
    }

    Ok(bootstrap_path)
}

fn write_nixpkgs_config(nixpkgs_config: &NixpkgsConfig, dotfile_dir: &Path) -> Result<PathBuf> {
    let nixpkgs_config_base = ser_nix::to_string(&NixpkgsConfigForNix::from(nixpkgs_config))
        .map_err(|e| miette!("Failed to serialize nixpkgs config: {}", e))?;
    let nixpkgs_config_nix = format!(
        r#"let
  cfg = {base};
  getName = pkg: (builtins.parseDrvName (pkg.name or pkg.pname or "")).name;
  unfreePackageError = pkg:
    let
      name = getName pkg;
    in
      throw ''
        devenv: package '${{name}}' has an unfree license.

        To allow all unfree packages, add this to devenv.yaml:

          allow_unfree: true

        To allow only this package, add this to devenv.yaml:

          nixpkgs:
            permitted_unfree_packages:
              - ${{name}}
      '';
in cfg // {{
  allowUnfreePredicate =
    if cfg.allowUnfree or false then
      (_: true)
    else if (cfg.permittedUnfreePackages or []) != [] then
      (pkg: builtins.elem (getName pkg) (cfg.permittedUnfreePackages or []) || unfreePackageError pkg)
    else
      unfreePackageError;
}}"#,
        base = nixpkgs_config_base
    );

    let config_hash = &compute_string_hash(&nixpkgs_config_nix)[..16];
    let nixpkgs_config_path = dotfile_dir.join(format!("nixpkgs-config-{}.nix", config_hash));
    std::fs::write(&nixpkgs_config_path, &nixpkgs_config_nix)
        .map_err(|e| miette!("Failed to write nixpkgs config: {}", e))?;
    Ok(nixpkgs_config_path)
}

pub fn apply_substituters_and_keys(store: &Store, store_settings: &StoreSettings) {
    let mut store = store.clone();
    for substituter in &store_settings.extra_substituters {
        if let Err(e) = store.add_substituter(substituter).to_miette() {
            tracing::warn!("Failed to add substituter {}: {}", substituter, e);
        }
    }
    if !store_settings.extra_trusted_public_keys.is_empty() {
        let keys: Vec<&str> = store_settings
            .extra_trusted_public_keys
            .iter()
            .map(String::as_str)
            .collect();
        if let Err(e) = store.add_trusted_public_keys(&keys).to_miette() {
            tracing::warn!("Failed to add trusted public keys: {}", e);
        }
    }
}

/// Whether the managed netrc has already been seeded this process, and how
/// that went. See [`apply_netrc_setting`].
static NETRC_PRESERVATION: OnceLock<NetrcPreservation> = OnceLock::new();

/// Switch Nix to devenv's managed netrc without dropping credentials for
/// substituters already configured in nix.conf. The managed file is created
/// by `CachixManager` before store initialization, then seeded from the
/// currently effective Nix `netrc-file` before the global setting changes.
fn apply_netrc_setting(store_settings: &StoreSettings) -> Result<()> {
    let Some(netrc) = &store_settings.netrc_path else {
        return Ok(());
    };
    let Some(netrc_str) = netrc.to_str() else {
        return Ok(());
    };

    // Seeding has to happen exactly once. This runs again from
    // `apply_store_settings`, where `preserve_netrc_file` normally
    // short-circuits because `netrc-file` already names the managed file --
    // but only if the `settings::set` below succeeded, and its failure is
    // warn-only in `init_nix`. Without this guard the retry would append a
    // second copy of every global credential.
    let preservation = match NETRC_PRESERVATION.get() {
        Some(preservation) => *preservation,
        None => {
            let existing_netrc = settings::get("netrc-file")
                .to_miette()
                .wrap_err("Failed to read the existing netrc-file setting")?;
            let preservation = if existing_netrc.is_empty() {
                NetrcPreservation::NothingToPreserve
            } else {
                preserve_netrc_file(Path::new(&existing_netrc), netrc)?
            };
            let _ = NETRC_PRESERVATION.set(preservation);
            preservation
        }
    };

    // Nix holds one netrc at a time. When the credentials in the current one
    // could not be copied across, pointing Nix at ours trades them away, so
    // wait until ours has entries of its own to offer in return: an empty
    // managed netrc would only turn authenticated fetches into 401s.
    if preservation == NetrcPreservation::SourceUnreadable && netrc_is_empty(netrc) {
        tracing::warn!(
            "Keeping the existing netrc-file: its credentials could not be read, \
             and devenv has none of its own to add yet"
        );
        return Ok(());
    }

    settings::set("netrc-file", netrc_str)
        .to_miette()
        .wrap_err("Failed to set netrc-file")?;
    Ok(())
}

fn netrc_is_empty(netrc: &Path) -> bool {
    match std::fs::metadata(netrc) {
        Ok(metadata) => metadata.len() == 0,
        // A netrc we cannot even stat has nothing to offer Nix either.
        Err(_) => true,
    }
}

pub fn apply_nix_settings(nix_settings: &NixSettings) -> Result<()> {
    settings::set("eval-cache", "false")
        .to_miette()
        .wrap_err("Failed to disable eval-cache")?;
    settings::set("always-allow-substitutes", "true")
        .to_miette()
        .wrap_err("Failed to set always-allow-substitutes")?;
    settings::set("http-connections", "100")
        .to_miette()
        .wrap_err("Failed to set http-connections")?;

    if nix_settings.offline {
        settings::set("substituters", "")
            .to_miette()
            .wrap_err("Failed to set offline mode (substituters)")?;
        settings::set("connect-timeout", "1")
            .to_miette()
            .wrap_err("Failed to set connect-timeout for offline mode")?;
    }
    if nix_settings.max_jobs > 0 {
        settings::set("max-jobs", &nix_settings.max_jobs.to_string())
            .to_miette()
            .wrap_err("Failed to set max-jobs")?;
    }
    if nix_settings.cores > 0 {
        settings::set("cores", &nix_settings.cores.to_string())
            .to_miette()
            .wrap_err("Failed to set cores")?;
    }
    if !nix_settings.system.is_empty() && nix_settings.system != "unknown architecture-unknown OS" {
        settings::set("system", &nix_settings.system)
            .to_miette()
            .wrap_err("Failed to set system")?;
    }
    if !nix_settings.impure {
        settings::set("pure-eval", "true")
            .to_miette()
            .wrap_err("Failed to set pure-eval mode")?;
        settings::set("pure-eval-allow-local-paths", "true")
            .to_miette()
            .wrap_err("Failed to set pure-eval-allow-local-paths")?;
    }
    settings::set("use-registries", "true")
        .to_miette()
        .wrap_err("Failed to set use-registries")?;
    settings::set("show-trace", "true")
        .to_miette()
        .wrap_err("Failed to set show-trace")?;

    for pair in nix_settings.nix_options.chunks_exact(2) {
        let key = &pair[0];
        let value = &pair[1];
        settings::set(key, value)
            .to_miette()
            .wrap_err(format!("Failed to set nix option: {key} = {value}"))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{
        cache_key_for, core_config_watch_paths, logical_store_path_str, write_nixpkgs_config,
    };
    use crate::primops::{AllocatePortPrimop, PrimopRegistry};
    use devenv_core::{PortAllocator, bootstrap_args::BootstrapArgs, config::NixpkgsConfig};
    use serde::Serialize;
    use tempfile::TempDir;

    #[derive(Serialize)]
    struct TinyArgs<'a> {
        version: &'a str,
    }

    #[test]
    fn core_config_watch_paths_only_tracks_existing_project_files() {
        let temp_dir = TempDir::new().expect("failed to create temp dir");
        std::fs::write(temp_dir.path().join("devenv.nix"), "{ ... }: { }").unwrap();
        std::fs::write(temp_dir.path().join("devenv.yaml"), "inputs: {}\n").unwrap();
        let lock_file = temp_dir.path().join("custom.lock");
        std::fs::write(&lock_file, "{}\n").unwrap();

        let tracked = core_config_watch_paths(temp_dir.path(), &lock_file);

        assert!(tracked.contains(&temp_dir.path().join("devenv.nix")));
        assert!(tracked.contains(&temp_dir.path().join("devenv.yaml")));
        assert!(tracked.contains(&lock_file));
        assert!(!tracked.contains(&temp_dir.path().join("devenv.local.nix")));
        assert!(!tracked.contains(&temp_dir.path().join("devenv.local.yaml")));
    }

    #[test]
    fn cache_key_reflects_current_port_allocator_mode() {
        let args = BootstrapArgs::from_serializable(&TinyArgs { version: "test" }).unwrap();
        let allocator = Arc::new(PortAllocator::new());
        let mut primops = PrimopRegistry::new();
        primops.add(AllocatePortPrimop::new(allocator.clone()));

        let shell_key = cache_key_for(&args, &primops, "shell");
        allocator.set_enabled(true);
        let up_key = cache_key_for(&args, &primops, "shell");
        allocator.set_strict(true);
        let strict_key = cache_key_for(&args, &primops, "shell");

        assert_ne!(shell_key.key_hash, up_key.key_hash);
        assert_ne!(up_key.key_hash, strict_key.key_hash);
    }

    #[test]
    fn nixpkgs_config_unfree_predicate_mentions_devenv_yaml_options() {
        let temp_dir = TempDir::new().expect("failed to create temp dir");
        let config_path = write_nixpkgs_config(&NixpkgsConfig::default(), temp_dir.path()).unwrap();
        let config = std::fs::read_to_string(config_path).unwrap();

        assert!(config.contains("devenv: package '${name}' has an unfree license."));
        assert!(config.contains("allow_unfree: true"));
        assert!(config.contains("permitted_unfree_packages:"));
        assert!(config.contains("else\n      unfreePackageError;"));
    }

    // Regression for devenv #2499: a shell built against a relocated/chroot store
    // caches the real_path-translated path; gc-root creation must still recover
    // the logical store path so `parse_store_path` accepts it.
    #[test]
    fn logical_store_path_str_recovers_logical_path_from_relocated_real_path() {
        let name = "rdd4pnr4x9rqc9wgbibhngv217w2xvxl-bash-interactive-5.2p26";
        let logical = format!("/nix/store/{name}");

        // Real path under a relocated store root maps back to the logical path.
        let real = format!("/srv/nix/store/{name}");
        assert_eq!(
            logical_store_path_str("/nix/store", &real).as_deref(),
            Some(logical.as_str())
        );

        // An already-logical path is unchanged.
        assert_eq!(
            logical_store_path_str("/nix/store", &logical).as_deref(),
            Some(logical.as_str())
        );

        // A trailing slash on the store dir is handled.
        assert_eq!(
            logical_store_path_str("/nix/store/", &real).as_deref(),
            Some(logical.as_str())
        );

        // A path with no basename yields None rather than a malformed path.
        assert_eq!(logical_store_path_str("/nix/store", "/"), None);
    }
}
