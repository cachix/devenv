//! C-Nix backend (`NixCBackend`).
//!
//! Owns the FFI primitives — store, eval state, settings, activity logger,
//! GC registration — plus devenv's intra-process eval caching, the cached
//! root devenv `Value`, and the port-allocator primop binding.
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
//! let fingerprint = {
//!     let lock_eval_state = build_lock_eval_state(&store, &root, &flake_settings)?;
//!     crate::lock::validate_and_load(&lock_eval_state, &store, &fetchers_settings,
//!         &flake_settings, &root, &inputs)?
//!     // lock_eval_state dropped here
//! };
//! let bootstrap_args = build_bootstrap_args(..., &fingerprint)?;
//! let backend = NixCBackend::new(
//!     paths, nix_settings, cache_settings, nixpkgs_config,
//!     store, flake_settings, fetchers_settings, _gc,
//!     bootstrap_args, port_allocator, eval_cache_pool,
//! )?;
//! ```

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use cstr::cstr;
use devenv_activity::{Activity, ActivityInstrument, activity, instrument_activity};
use devenv_cache_core::compute_string_hash;
use devenv_core::bootstrap_args::BootstrapArgs;
use devenv_core::config::NixpkgsConfig;
use devenv_core::evaluator::{
    BuildOptions, DevEnvOutput, Evaluator, NixMetadata, PackageSearchResult, SearchResults,
};
use devenv_core::nix_backend::eval_cache_key_args;
use devenv_core::nix_log_bridge::{EvalActivityGuard, NixLogBridge};
use devenv_core::realized::RealizedPathsObserver;
use devenv_core::store::Store as StoreTrait;
use devenv_core::store::StorePath as CoreStorePath;
use devenv_core::{CacheSettings, DevenvPaths, NixSettings, PortAllocator, StoreSettings};
use devenv_eval_cache::{
    self, CachedEval, CachingConfig, CachingEvalService, CachingEvalState, EvalCacheKey,
    ResourceManager,
};
use miette::{IntoDiagnostic, Result, WrapErr, bail, miette};
use nix_bindings_expr::eval_state::{
    EvalState, EvalStateBuilder, ThreadRegistrationGuard, gc_register_my_thread,
};
use nix_bindings_expr::primop::{PrimOp, PrimOpMeta};
use nix_bindings_expr::to_json::value_to_json;
use nix_bindings_expr::{EvalCache, SearchParams, SearchResult, search};
use nix_bindings_fetchers::FetchersSettings;
use nix_bindings_flake::{EvalStateBuilderExt, FlakeSettings};
use nix_bindings_store::build_env::BuildEnvironment;
use nix_bindings_store::store::{GcAction, Store, TrustedFlag};
use nix_bindings_util::settings;
use nix_cmd::ReplExitStatus;
use once_cell::sync::OnceCell;

use crate::anyhow_ext::AnyhowToMiette;
use crate::build_environment::BuildEnvironment as RustBuildEnvironment;
use crate::cnix_store::CNixStore;
use crate::error::dedent_lines;
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
    settings::set("experimental-features", "flakes nix-command")
        .to_miette()
        .wrap_err("Failed to enable experimental features")?;
    apply_nix_settings(nix_settings)?;
    if let Some(netrc) = &store_settings.netrc_path
        && let Some(s) = netrc.to_str()
    {
        settings::set("netrc-file", s)
            .to_miette()
            .wrap_err("Failed to set netrc-file")?;
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

/// Build a transient `EvalState` for lock-file work. Caller drops it
/// once locking is finished — it is *not* the long-lived eval state the
/// backend uses for evaluation.
pub fn build_lock_eval_state(
    store: &Store,
    root: &Path,
    flake_settings: &FlakeSettings,
) -> Result<EvalState> {
    let root_str = root
        .to_str()
        .ok_or_else(|| miette!("Root path contains invalid UTF-8"))?;
    EvalStateBuilder::new(store.clone())
        .to_miette()
        .wrap_err("Failed to create eval state builder")?
        .base_directory(root_str)
        .to_miette()
        .wrap_err("Failed to set base directory")?
        .flakes(flake_settings)
        .to_miette()
        .wrap_err("Failed to configure flakes")?
        .build()
        .to_miette()
        .wrap_err("Failed to build eval state")
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
    port_allocator: Arc<PortAllocator>,

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

fn core_config_watch_paths(root: &Path) -> Vec<PathBuf> {
    [
        "devenv.nix",
        "devenv.yaml",
        "devenv.lock",
        "devenv.local.nix",
        "devenv.local.yaml",
    ]
    .into_iter()
    .map(|path| root.join(path))
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

fn cache_key_for(
    bootstrap_args: &BootstrapArgs,
    port_allocator: &PortAllocator,
    attr_name: &str,
) -> EvalCacheKey {
    let cache_key_args = eval_cache_key_args(
        bootstrap_args.as_str(),
        port_allocator.is_enabled(),
        port_allocator.is_strict(),
    );
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
        port_allocator: Arc<PortAllocator>,
        eval_cache_pool: Option<Arc<tokio::sync::OnceCell<sqlx::SqlitePool>>>,
        logger_setup: crate::logger::NixLoggerSetup,
    ) -> Result<Self> {
        let bootstrap_path = extract_bootstrap_files(&paths.dotfile)?;
        let nixpkgs_config_path = write_nixpkgs_config(nixpkgs_config, &paths.dotfile)?;

        let eval_state = build_eval_state(
            &store,
            &paths.root,
            &nixpkgs_config_path,
            &flake_settings,
            nix_settings.nix_debugger,
        )?;

        let activity_logger = logger_setup.logger;
        let nix_log_bridge = logger_setup.bridge;

        let cnix_store = CNixStore::new(store);

        let backend = Self {
            nix_settings,
            cache_settings,
            paths,
            bootstrap_path,
            nixpkgs_config_path,
            nix_log_bridge,
            eval_cache_pool,
            bootstrap_args,
            port_allocator,
            cached_devenv_value: Mutex::new(None),
            devenv_value_invalidated: Arc::new(AtomicBool::new(false)),
            caching_eval_state: OnceCell::new(),
            eval_state: Arc::new(Mutex::new(Some(eval_state))),
            cnix_store,
            flake_settings,
            fetchers_settings,
            activity_logger,
            realized_observers: Mutex::new(Vec::new()),
            _gc_registration: gc_registration,
        };
        backend.init_caching_eval_state();
        Ok(backend)
    }

    fn init_caching_eval_state(&self) {
        if self.caching_eval_state.get().is_some() {
            return;
        }
        let cache_key_args = eval_cache_key_args(
            self.bootstrap_args.as_str(),
            self.port_allocator.is_enabled(),
            self.port_allocator.is_strict(),
        );

        let cached_eval = if let Some(pool_cell) = &self.eval_cache_pool {
            if let Some(pool) = pool_cell.get() {
                let config = CachingConfig {
                    force_refresh: self.cache_settings.refresh_eval_cache,
                    extra_watch_paths: core_config_watch_paths(&self.paths.root),
                    excluded_envs: vec!["NIXPKGS_CONFIG".to_string()],
                    excluded_paths: vec![self.nixpkgs_config_path.clone()],
                };
                let service = CachingEvalService::with_config(pool.clone(), config.clone());
                let invalidation_flag = self.devenv_value_invalidated.clone();
                let resource_manager = Arc::new(ResourceManager::new(self.port_allocator.clone()));
                CachedEval::with_cache(service, self.nix_log_bridge.clone(), config)
                    .with_resource_manager(resource_manager)
                    .with_on_resource_invalidation(Arc::new(move || {
                        invalidation_flag.store(true, Ordering::Release);
                    }))
            } else {
                CachedEval::without_cache(self.nix_log_bridge.clone())
            }
        } else {
            CachedEval::without_cache(self.nix_log_bridge.clone())
        };

        let caching_eval_state =
            CachingEvalState::new(self.eval_state.clone(), cached_eval, cache_key_args);
        let _ = self.caching_eval_state.set(caching_eval_state);
    }

    fn cache_key(&self, attr_name: &str) -> EvalCacheKey {
        cache_key_for(&self.bootstrap_args, &self.port_allocator, attr_name)
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
        let eval_activity = self.nix_log_bridge.begin_eval(activity.id());
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

        let primops_attrset = if self.port_allocator.is_enabled() {
            let port_allocator = self.port_allocator.clone();
            let primop = PrimOp::new(
                eval_state,
                PrimOpMeta {
                    name: cstr!("allocatePort"),
                    doc: cstr!("Allocate a free port starting from base"),
                    args: [cstr!("processName"), cstr!("portName"), cstr!("basePort")],
                },
                Box::new(move |es, [process_name, port_name, base_port]| {
                    let process = es.require_string(process_name)?;
                    let port_name_str = es.require_string(port_name)?;
                    let base_raw = es.require_int(base_port)?;
                    let base = u16::try_from(base_raw).map_err(|_| {
                        anyhow::anyhow!("basePort must be between 0 and 65535, got {}", base_raw)
                    })?;
                    let allocated = port_allocator
                        .allocate(&process, &port_name_str, base)
                        .map_err(|e| anyhow::anyhow!("{}", e))?;
                    es.new_value_int(allocated as i64)
                }),
            )
            .to_miette()
            .wrap_err("Failed to create allocatePort primop")?;

            let primop_value = eval_state
                .new_value_primop(primop)
                .to_miette()
                .wrap_err("Failed to create primop value")?;
            eval_state
                .new_value_attrs(vec![("allocatePort".to_string(), primop_value)])
                .to_miette()
                .wrap_err("Failed to create primops attrset")?
        } else {
            eval_state
                .new_value_attrs(vec![])
                .to_miette()
                .wrap_err("Failed to create empty primops attrset")?
        };

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
        // Flatten into a single diagnostic. Nix already emits a complete
        // tree-style trace; letting miette render the FFI cause chain on top
        // of that produces deep continuation indent under `─▶` arrows.
        let nix_errors = self.nix_log_bridge.peek_pre_repl_errors();
        let raw = nix_errors
            .last()
            .cloned()
            .unwrap_or_else(|| format!("{err:#}"));
        miette!("{context}: {}", dedent_lines(&raw))
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
            format!(
                "Failed to get attribute '{}' from devenv configuration",
                attr_path
            ),
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
        let mut eval_state = self.eval_session(activity)?;
        let devenv = self.get_or_eval_devenv(&mut eval_state)?;

        let shell_drv = self.enriched(
            eval_state.require_attrs_select(&devenv, "shell"),
            "Failed to get shell attribute from devenv",
        )?;
        self.enriched(
            eval_state.force(&shell_drv),
            "Failed to force evaluation of shell derivation",
        )?;

        let drv_path_value = self.enriched(
            eval_state.require_attrs_select(&shell_drv, "drvPath"),
            "Failed to get drvPath from shell derivation",
        )?;
        let drv_path = self.enriched(
            eval_state.require_string(&drv_path_value),
            "Failed to extract drvPath as string",
        )?;

        let out_path_value = self.enriched(
            eval_state.require_attrs_select(&shell_drv, "outPath"),
            "Failed to get outPath from shell derivation",
        )?;

        let realized = {
            let _guard = UmaskGuard::restrictive();
            self.enriched(
                eval_state.realise_string(&out_path_value, false),
                "Failed to realize shell derivation",
            )?
        };

        let store_path = realized
            .paths
            .first()
            .ok_or_else(|| miette!("Shell derivation produced no output paths"))?;

        let mut store = self.cnix_store.inner().clone();
        let out_path = store
            .real_path(store_path)
            .to_miette()
            .wrap_err("Failed to get store path")?;

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

        let env_path = Some(
            store
                .real_path(&env_store_path)
                .to_miette()
                .wrap_err("Failed to get env store path")?,
        );

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
            format!(
                "Failed to get attribute '{}' from devenv configuration",
                attr_path
            ),
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
        let caching_state = self
            .caching_eval_state
            .get()
            .expect("caching eval state must be initialized");

        let cache_key = self.cache_key("shell");

        let cached_paths: Option<CachedShellPaths> = if let Some(service) =
            caching_state.cached_eval().service()
        {
            match service.get_cached(&cache_key).await {
                Ok(Some(cached)) => {
                    match serde_json::from_str::<CachedShellPaths>(&cached.json_output) {
                        Ok(paths) => {
                            let drv_exists = std::path::Path::new(&paths.drv_path).exists();
                            let out_exists = std::path::Path::new(&paths.out_path).exists();
                            if drv_exists && out_exists {
                                match caching_state
                                    .cached_eval()
                                    .try_replay_resources(cached.eval_id)
                                    .await
                                {
                                    Ok(()) => Some(paths),
                                    Err(e) => {
                                        tracing::warn!(error = %e, "Resource replay failed for shell cache hit, re-evaluating");
                                        if let Some(svc) = caching_state.cached_eval().service()
                                            && let Err(db_err) =
                                                svc.invalidate_resource_dependent().await
                                        {
                                            tracing::warn!(error = %db_err, "Failed to delete port-dependent cache entries");
                                        }
                                        caching_state.cached_eval().clear_resources();
                                        if let Ok(mut cached) = self.cached_devenv_value.lock() {
                                            *cached = None;
                                        }
                                        None
                                    }
                                }
                            } else {
                                if let Err(db_err) = service.invalidate(&cache_key).await {
                                    tracing::warn!(error = %db_err, "Failed to invalidate stale shell cache entry");
                                }
                                None
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "Failed to parse cached shell paths");
                            None
                        }
                    }
                }
                Ok(None) => None,
                Err(e) => {
                    tracing::warn!(error = %e, "Error checking eval cache for shell");
                    None
                }
            }
        } else {
            None
        };

        let activity = activity!(INFO, evaluate, "Evaluating shell");

        let (drv_path_str, out_path_str, env_path, cache_hit) = if let Some(paths) = cached_paths {
            activity.cached();
            (paths.drv_path, paths.out_path, paths.env_path, true)
        } else {
            let result = async {
                caching_state
                    .cached_eval()
                    .eval_typed::<CachedShellPaths, _, _, _>(&cache_key, &activity, || async {
                        self.build_shell_uncached(&activity)
                    })
                    .await
            }
            .in_activity(&activity)
            .await;

            match result {
                Ok((paths, _)) => (paths.drv_path, paths.out_path, paths.env_path, false),
                Err(e) => {
                    activity.fail();
                    return Err(eval_cache_error_into_miette(e));
                }
            }
        };

        let mut store = self.cnix_store.inner().clone();
        let store_path = store
            .parse_store_path(&out_path_str)
            .to_miette()
            .wrap_err("Failed to parse output store path")?;

        if gc_root.symlink_metadata().is_ok() {
            std::fs::remove_file(gc_root)
                .map_err(|e| miette!("Failed to remove existing GC root: {}", e))?;
        }
        store
            .add_perm_root(&store_path, gc_root)
            .to_miette()
            .wrap_err("Failed to create GC root")?;

        if !cache_hit {
            self.notify_realized(&[PathBuf::from(&out_path_str)]);
        }

        let output_str = if let Some(env_path) = env_path.as_deref() {
            if std::path::Path::new(env_path).exists() {
                let env_json = std::fs::read_to_string(env_path)
                    .into_diagnostic()
                    .wrap_err("Failed to read cached env JSON")?;
                let rust_env = RustBuildEnvironment::from_json(&env_json)
                    .into_diagnostic()
                    .wrap_err("Failed to parse cached env JSON")?;

                if json {
                    env_json
                } else {
                    rust_env.to_activation_script()
                }
            } else {
                self.build_dev_environment(&mut store, &drv_path_str, json)?
            }
        } else {
            self.build_dev_environment(&mut store, &drv_path_str, json)?
        };

        let inputs = if let Some(service) = caching_state.cached_eval().service() {
            service
                .get_file_inputs(&cache_key)
                .await
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        Ok(DevEnvOutput {
            bash_env: output_str.as_bytes().to_vec(),
            inputs,
        })
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
                .wrap_err("Failed to get pkgs attribute from devenv")?;
            env.insert("pkgs", &pkgs)
                .to_miette()
                .wrap_err("Failed to inject pkgs into REPL scope")?;
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
            "Failed to get pkgs attribute from devenv configuration",
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

    pub async fn metadata(&self) -> Result<NixMetadata> {
        let lock_file_path = self.paths.root.join("devenv.lock");
        let inputs_section = if lock_file_path.exists() {
            let lock = crate::load_lock_file(&self.fetchers_settings, &lock_file_path)
                .to_miette()
                .wrap_err("Failed to load lock file")?;
            if let Some(lock_file) = lock {
                format_lock_inputs(&lock_file)?
            } else {
                "Inputs:\n  (no lock file)".to_string()
            }
        } else {
            "Inputs:\n  (no lock file)".to_string()
        };

        let info_str = match self.eval(&["config.info"]).await {
            Ok(json_str) => serde_json::from_str::<String>(&json_str).unwrap_or_default(),
            Err(_) => String::new(),
        };

        if info_str.is_empty() {
            Ok(inputs_section)
        } else {
            Ok(format!("{inputs_section}\n\n{info_str}"))
        }
    }

    pub async fn gc(&self, paths: Vec<PathBuf>) -> Result<(u64, u64)> {
        if paths.is_empty() {
            return Ok((0, 0));
        }

        let mut store = self.cnix_store.inner().clone();
        let mut total_deleted = 0u64;
        let mut total_bytes_freed = 0u64;
        let total_paths = paths.len() as u64;

        let activity = activity!(INFO, operation, "Deleting store paths");

        for (i, path) in paths.iter().enumerate() {
            let path_str = match path.to_str() {
                Some(s) => s,
                None => continue,
            };
            let path_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(path_str);
            activity.progress(i as u64, total_paths, Some(path_name));

            let store_path = match store.parse_store_path(path_str).to_miette() {
                Ok(sp) => sp,
                Err(_) => {
                    let _ = std::fs::remove_file(path).or_else(|_| std::fs::remove_dir_all(path));
                    continue;
                }
            };

            match store.collect_garbage(GcAction::DeleteSpecific, Some(&[&store_path]), false, 0) {
                Ok((deleted, bytes_freed)) => {
                    total_deleted += deleted.len() as u64;
                    total_bytes_freed += bytes_freed;
                }
                Err(_) => continue,
            }
        }

        activity.progress(total_paths, total_paths, None);
        Ok((total_deleted, total_bytes_freed))
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

    /// Apply substituters and trusted public keys to the open store.
    ///
    /// Use after backend init when the cachix configuration has been
    /// evaluated (the `netrc-file` global setting is the one piece that
    /// must land before the store opens; everything else is additive
    /// and can be applied here). Failures are logged warn but never
    /// fatal — devenv continues without the cachix substituters.
    pub fn apply_store_settings(&self, store_settings: &StoreSettings) {
        // Open an eval scope on the bridge so substituter info fetches
        // (e.g. `nix-cache-info` downloads) fired from worker threads
        // inside the C call nest under the current TUI activity.
        let _eval_guard =
            devenv_activity::current_activity_id().map(|id| self.nix_log_bridge.begin_eval(id));
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
            state.cached_eval().input_tracker().clear();
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
        )?;
        *guard = Some(new_state);

        Ok(())
    }

    #[cfg(feature = "test-nix-store")]
    pub fn log_bridge(&self) -> &Arc<NixLogBridge> {
        &self.nix_log_bridge
    }

    #[cfg(feature = "test-nix-store")]
    pub fn input_tracker(&self) -> Option<&Arc<devenv_eval_cache::InputTracker>> {
        self.caching_eval_state
            .get()
            .map(|state| state.cached_eval().input_tracker())
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
                                    .try_replay_resources(cached.eval_id)
                                    .await
                                {
                                    Ok(()) => Some(path_str),
                                    Err(e) => {
                                        tracing::warn!(error = %e, attr_path = attr_path, "Resource replay failed for build cache hit, re-evaluating");
                                        if let Err(db_err) =
                                            service.invalidate_resource_dependent().await
                                        {
                                            tracing::warn!(error = %db_err, "Failed to delete port-dependent cache entries");
                                        }
                                        caching_state.cached_eval().clear_resources();
                                        if let Ok(mut cached) = self.cached_devenv_value.lock() {
                                            *cached = None;
                                        }
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
                let store_path = store
                    .parse_store_path(&path_str)
                    .to_miette()
                    .wrap_err("Failed to parse store path")?;

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

fn format_lock_inputs(lock_file: &nix_bindings_flake::LockFile) -> Result<String> {
    let mut iter = lock_file
        .inputs_iterator()
        .to_miette()
        .wrap_err("Failed to create inputs iterator")?;

    let mut inputs = Vec::new();
    while iter.next() {
        let attr_path = iter
            .attr_path()
            .to_miette()
            .wrap_err("Failed to get attr path")?;
        let locked_ref = iter
            .locked_ref()
            .to_miette()
            .wrap_err("Failed to get locked ref")?;
        if !attr_path.contains('/') {
            inputs.push((attr_path, locked_ref));
        }
    }

    if inputs.is_empty() {
        return Ok("Inputs:\n  (no inputs)".to_string());
    }

    inputs.sort_by(|a, b| a.0.cmp(&b.0));

    let mut lines = vec!["Inputs:".to_string()];
    let inputs_len = inputs.len();
    for (idx, (path, ref_str)) in inputs.into_iter().enumerate() {
        let is_last = idx == inputs_len - 1;
        let prefix = if is_last {
            "└───"
        } else {
            "├───"
        };
        lines.push(format!(
            "{prefix}{path}: {brief}",
            brief = format_brief_ref(&ref_str)
        ));
    }
    Ok(lines.join("\n"))
}

fn format_brief_ref(ref_str: &str) -> String {
    if ref_str.is_empty() {
        return String::from("(follows)");
    }
    if let Some(last_slash_idx) = ref_str.rfind('/') {
        let before_slash = &ref_str[..last_slash_idx];
        let after_slash = &ref_str[last_slash_idx + 1..];
        if after_slash.len() >= 40 && after_slash.chars().all(|c| c.is_ascii_hexdigit()) {
            return format!("{}/{}", before_slash, &after_slash[..7]);
        }
    }
    ref_str.to_string()
}

fn build_eval_state(
    store: &Store,
    root: &Path,
    nixpkgs_config_path: &Path,
    flake_settings: &FlakeSettings,
    enable_debugger: bool,
) -> Result<EvalState> {
    let root_str = root
        .to_str()
        .ok_or_else(|| miette!("Root path contains invalid UTF-8"))?;
    let nixpkgs_config_str = nixpkgs_config_path
        .to_str()
        .ok_or_else(|| miette!("Nixpkgs config path contains invalid UTF-8"))?;

    let mut eval_state = EvalStateBuilder::new(store.clone())
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
        .wrap_err("Failed to configure flakes")?
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
    let nixpkgs_config_base = ser_nix::to_string(nixpkgs_config)
        .map_err(|e| miette!("Failed to serialize nixpkgs config: {}", e))?;
    let nixpkgs_config_nix = format!(
        r#"let
  cfg = {base};
  getName = pkg: (builtins.parseDrvName (pkg.name or pkg.pname or "")).name;
in cfg // {{
  allowUnfreePredicate =
    if cfg.allowUnfree or false then
      (_: true)
    else if (cfg.permittedUnfreePackages or []) != [] then
      (pkg: builtins.elem (getName pkg) (cfg.permittedUnfreePackages or []))
    else
      (_: false);
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
    if nix_settings.refresh_fetchers {
        settings::set("tarball-ttl", "0")
            .to_miette()
            .wrap_err("Failed to set tarball-ttl")?;
    }
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
    use super::{cache_key_for, core_config_watch_paths};
    use devenv_core::{PortAllocator, bootstrap_args::BootstrapArgs};
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
        std::fs::write(temp_dir.path().join("devenv.lock"), "{}\n").unwrap();

        let tracked = core_config_watch_paths(temp_dir.path());

        assert!(tracked.contains(&temp_dir.path().join("devenv.nix")));
        assert!(tracked.contains(&temp_dir.path().join("devenv.yaml")));
        assert!(tracked.contains(&temp_dir.path().join("devenv.lock")));
        assert!(!tracked.contains(&temp_dir.path().join("devenv.local.nix")));
        assert!(!tracked.contains(&temp_dir.path().join("devenv.local.yaml")));
    }

    #[test]
    fn cache_key_reflects_current_port_allocator_mode() {
        let args = BootstrapArgs::from_serializable(&TinyArgs { version: "test" }).unwrap();
        let allocator = PortAllocator::new();

        let shell_key = cache_key_for(&args, &allocator, "shell");
        allocator.set_enabled(true);
        let up_key = cache_key_for(&args, &allocator, "shell");
        allocator.set_strict(true);
        let strict_key = cache_key_for(&args, &allocator, "shell");

        assert_ne!(shell_key.key_hash, up_key.key_hash);
        assert_ne!(up_key.key_hash, strict_key.key_hash);
    }
}
