//! FFI-based NixBackend implementation using direct Rust bindings to Nix C++.
//!
//! This module implements the devenv NixBackend trait using the nixops4 Rust crates
//! which provide FFI bindings to the Nix C++ libraries. This replaces the shell-based
//! approach of spawning `nix` command processes with direct library calls.
//!
//! This version evaluates a plain default.nix file while using resolve-lock.nix to
//! resolve inputs from devenv.lock

use crate::anyhow_ext::AnyhowToMiette;
use crate::build_environment::BuildEnvironment as RustBuildEnvironment;

use async_trait::async_trait;
use cstr::cstr;
use include_dir::{Dir, include_dir};
use tokio_shutdown::Shutdown;

use devenv_activity::{Activity, ActivityInstrument, ActivityLevel};
use devenv_cache_core::compute_string_hash;
use devenv_core::GlobalOptions;
use devenv_core::PortAllocator;
use devenv_core::cachix::{CachixCacheInfo, CachixConfig, CachixManager};
use devenv_core::config::Config;
use devenv_core::nix_args::NixArgs;
use devenv_core::nix_backend::{
    DevEnvOutput, DevenvPaths, NixBackend, Options, PackageSearchResult, SearchResults,
};
use devenv_core::nix_log_bridge::{EvalActivityGuard, NixLogBridge};
use devenv_eval_cache::{
    CacheError, CachedEval, CachingConfig, CachingEvalService, CachingEvalState, ResourceManager,
};
use miette::{IntoDiagnostic, Result, WrapErr, miette};
use nix_bindings_expr::eval_state::{EvalState, EvalStateBuilder, gc_register_my_thread};
use nix_bindings_expr::primop::{PrimOp, PrimOpMeta};
use nix_bindings_expr::to_json::value_to_json;
use nix_bindings_expr::{EvalCache, SearchParams, SearchResult, search};
use nix_bindings_fetchers::FetchersSettings;
use nix_bindings_flake::{
    EvalStateBuilderExt, FlakeReference, FlakeReferenceParseFlags, FlakeSettings, InputsLocker,
    LockMode,
};
use nix_bindings_store::build_env::BuildEnvironment;
use nix_bindings_store::store::{GcAction, Store, TrustedFlag};
use nix_bindings_util::settings;
use nix_cmd::ReplExitStatus;
use once_cell::sync::OnceCell;
use ser_nix;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Embedded bootstrap directory containing default.nix and resolve-lock.nix
static BOOTSTRAP_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/bootstrap");

/// Convert CacheError to miette::Error for Result compatibility
fn cache_error_to_miette(e: CacheError) -> miette::Error {
    miette!("{}", e)
}

/// Specifies where the project root is located
///
/// The project root is used as the base for loading devenv.nix and other configuration.
/// It can be either a local filesystem path or a reference to a flake input.
#[derive(Debug, Clone)]
pub enum ProjectRoot {
    /// A filesystem path (local or absolute)
    /// Creates a "project_root" input in the flake that gets locked and fetched
    /// Examples: PathBuf::from("."), PathBuf::from("/home/user/project")
    Path(PathBuf),

    /// A reference to a flake input, optionally with a subpath
    /// Examples: "nixpkgs", "myinput/subdir"
    /// The referenced input must already exist in devenv.yaml inputs
    InputRef(String),
}

impl Default for ProjectRoot {
    fn default() -> Self {
        ProjectRoot::Path(PathBuf::from("."))
    }
}

/// FFI-based Nix backend implementation using direct Rust bindings
///
/// Field order matters: Rust drops fields in declaration order. FFI fields are
/// ordered at the end so C++ destructors run in the correct dependency order:
/// caching_eval_state → eval_state → store → settings → activity_logger → _gc_registration
pub struct NixRustBackend {
    pub global_options: GlobalOptions,
    pub paths: DevenvPaths,
    pub config: Config,

    // Path to extracted bootstrap directory (lives for duration of backend)
    bootstrap_path: PathBuf,

    // Path to generated nixpkgs config file (for cache exclusion)
    nixpkgs_config_path: PathBuf,

    // Bridge for tracking eval activities dynamically and input collection
    // Used by eval_session() to create/complete eval activities
    // Also used for caching via observers
    nix_log_bridge: Arc<NixLogBridge>,

    // Optional eval cache pool from framework layer (shared with other backends)
    // Note: Uses tokio::sync::OnceCell to match the framework layer type
    eval_cache_pool: Option<Arc<tokio::sync::OnceCell<sqlx::SqlitePool>>>,

    // Flag to force cache bypass on next operation (for hot-reload)
    // Set by invalidate(), checked and cleared by dev_env()
    cache_invalidated: AtomicBool,

    // Cachix manager for handling binary cache configuration
    cachix_manager: Arc<CachixManager>,

    // Cachix daemon for pushing store paths (only when cachix.push is configured)
    cachix_daemon: Arc<tokio::sync::Mutex<Option<crate::cachix_daemon::OwnedDaemon>>>,

    // Activity for the cachix push operation (visible in TUI)
    cachix_activity: Arc<tokio::sync::Mutex<Option<Activity>>>,

    // Cached import expression: (import /path/to/default.nix { ... args ... })
    // Set once in assemble(). Kept for potential future use (e.g., debugging).
    #[allow(dead_code)]
    cached_import_expr: Arc<OnceCell<String>>,

    // Pre-serialized NixArgs for Value-based evaluation with primop injection.
    // Set once in assemble(), used by eval_import_with_primops() to build
    // the args attrset and merge it with the primops attrset.
    cached_args_nix_eval: Arc<OnceCell<String>>,

    // Shutdown coordinator - stored so we can trigger shutdown in Drop
    // This ensures cleanup tasks are signaled to exit when the backend is dropped
    shutdown: Arc<Shutdown>,

    // Port allocator for managing automatic port allocation during evaluation
    // Shared with eval cache for resource replay on cache hits
    port_allocator: Arc<PortAllocator>,

    // Caching wrapper around EvalState (drops first — releases Arc to eval_state)
    caching_eval_state: OnceCell<CachingEvalState<Arc<Mutex<EvalState>>>>,

    // EvalState (drops after caching wrapper; its C++ destructor may reference the store)
    eval_state: Arc<Mutex<EvalState>>,

    // Store (EvalState destructor may reference it)
    #[allow(dead_code)]
    store: Arc<Store>,

    // Settings (must outlive EvalState)
    #[allow(dead_code)]
    flake_settings: FlakeSettings,
    #[allow(dead_code)]
    fetchers_settings: FetchersSettings,

    // Activity logger (must outlive EvalState)
    activity_logger: nix_bindings_expr::logger::ActivityLogger,

    // GC thread registration (must be last FFI field to drop)
    #[allow(dead_code)]
    _gc_registration: nix_bindings_expr::eval_state::ThreadRegistrationGuard,
}

// SAFETY: This is unsafe and relies on several assumptions about the Nix C++ library:
//
// CRITICAL ASSUMPTIONS:
// 1. Store: The C++ nix::Store object and its internal nix::Context must be safe to
//    send between threads and access from multiple threads. We assume the Nix C++ library
//    uses appropriate locking internally for store operations.
//
// 2. EvalState: The C++ nix::EvalState is NOT thread-safe internally. We protect it with
//    a Mutex to ensure only one thread can access it at a time. This relies on the Mutex
//    providing proper synchronization.
//
// KNOWN RISKS:
// - The Nix C++ library is primarily designed for single-threaded CLI usage
// - If the C++ store implementation has thread-local state, this could break
// - If multiple threads trigger evaluation concurrently, the Mutex will serialize access
//   but there's no guarantee the C++ side doesn't have issues with this pattern
//
// RECOMMENDATION:
// - DO NOT use this backend from multiple threads concurrently if you can avoid it
// - Consider creating separate NixRustBackend instances per thread instead
// - This implementation exists to satisfy the NixBackend: Send + Sync trait bound
//
// TODO: Verify these assumptions by:
// 1. Reading Nix C++ source code for Store and EvalState thread safety
// 2. Adding integration tests that exercise concurrent access
// 3. Consider upstreaming Send/Sync impls to nix-store and nix-expr crates with proper safety analysis
unsafe impl Send for NixRustBackend {}
unsafe impl Sync for NixRustBackend {}

/// RAII guard that manages eval_state lock and evaluation activity tracking.
///
/// When created (via `eval_session(activity)`):
/// 1. Acquires the eval_state mutex lock
/// 2. Calls `bridge.begin_eval(activity.id())` to register the activity for file logging
///
/// When dropped:
/// 1. The `EvalActivityGuard` clears the activity ID via its Drop impl
/// 2. Releases the eval_state lock
///
/// The caller owns the Activity and controls its lifecycle. This guard just
/// ensures file evaluations are logged to the correct activity.
pub(crate) struct EvalSession<'a> {
    guard: std::sync::MutexGuard<'a, EvalState>,
    _eval_activity: EvalActivityGuard<'a>,
}

impl<'a> std::ops::Deref for EvalSession<'a> {
    type Target = EvalState;
    fn deref(&self) -> &Self::Target {
        &self.guard
    }
}

impl<'a> std::ops::DerefMut for EvalSession<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.guard
    }
}

impl NixRustBackend {
    /// Create a new NixRustBackend with initialized Nix FFI components with GlobalOptions
    ///
    /// # Arguments
    /// * `paths` - DevenvPaths containing root, dotfile, and cache directories
    /// * `config` - Devenv configuration
    /// * `global_options` - Global Nix options (offline mode, max jobs, etc.)
    /// * `cachix_manager` - CachixManager for handling binary cache configuration
    /// * `shutdown` - Shutdown coordinator for graceful cleanup of cachix daemon
    /// * `eval_cache_pool` - Optional eval cache database pool from framework layer
    /// * `store` - Optional custom Nix store path (for testing with restricted permissions)
    /// * `port_allocator` - Port allocator for managing automatic port allocation during evaluation
    pub fn new(
        paths: DevenvPaths,
        config: Config,
        global_options: GlobalOptions,
        cachix_manager: Arc<CachixManager>,
        shutdown: Arc<Shutdown>,
        eval_cache_pool: Option<Arc<tokio::sync::OnceCell<sqlx::SqlitePool>>>,
        store: Option<std::path::PathBuf>,
        port_allocator: Arc<PortAllocator>,
    ) -> Result<Self> {
        // Initialize Nix libexpr - uses Once internally so safe to call multiple times.
        // This may already have been called by worker threads via gc_register_current_thread().
        crate::nix_init();

        // Register thread with garbage collector IMMEDIATELY after init()
        // This MUST happen before any other Nix FFI calls (including settings::set)
        // because the Boehm GC requires thread registration before allocations.
        let gc_registration = gc_register_my_thread()
            .to_miette()
            .wrap_err("Failed to register thread with Nix garbage collector")?;

        // Set experimental features after init() to ensure they're properly configured
        settings::set("experimental-features", "flakes nix-command")
            .to_miette()
            .wrap_err("Failed to enable experimental features")?;

        // Apply other global settings
        Self::apply_global_options(&global_options)?;

        // Apply cachix global settings BEFORE store creation (e.g., netrc-file path)
        let cachix_global_settings = cachix_manager
            .get_global_settings()
            .wrap_err("Failed to get cachix global settings")?;

        for (key, value) in &cachix_global_settings {
            settings::set(key, value).to_miette().wrap_err(format!(
                "Failed to set cachix global setting: {} = {}",
                key, value
            ))?;
            tracing::debug!("Applied cachix global setting: {} = {}", key, value);
        }

        // Extract bootstrap directory to dotfile location
        let bootstrap_path = Self::extract_bootstrap_files(&paths.dotfile)?;

        // Open store connection
        let store_uri = store
            .as_ref()
            .map(|p| format!("local?root={}", p.display()));
        let store = Store::open(store_uri.as_deref(), [])
            .to_miette()
            .wrap_err("Failed to open Nix store")?;

        // Create flake and fetchers settings - kept alive for the entire backend lifetime
        // These are needed for builtins.fetchTree, builtins.getFlake, and flake operations
        // Note: Must be created after Store::open()
        let flake_settings = FlakeSettings::new()
            .to_miette()
            .wrap_err("Failed to create flake settings")?;
        // Note: Must be created after Store::open()
        let fetchers_settings = FetchersSettings::new()
            .to_miette()
            .wrap_err("Failed to create fetchers settings")?;

        // Generate merged nixpkgs config and write to temp file for NIXPKGS_CONFIG env var
        // Wrap in a let expression that adds allowUnfreePredicate (a Nix function)
        // Note: NIXPKGS_CONFIG expects a file path, not inline Nix content
        let nixpkgs_config = config.nixpkgs_config(&global_options.system);
        let nixpkgs_config_base = ser_nix::to_string(&nixpkgs_config)
            .map_err(|e| miette::miette!("Failed to serialize nixpkgs config: {}", e))?;
        let nixpkgs_config_nix = format!(
            r#"let cfg = {base}; in cfg // {{
  allowUnfreePredicate =
    if cfg.allowUnfree or false then
      (_: true)
    else if (cfg.permittedUnfreePackages or []) != [] then
      (pkg: builtins.elem ((builtins.parseDrvName (pkg.name or pkg.pname or pkg)).name) (cfg.permittedUnfreePackages or []))
    else
      (_: false);
}}"#,
            base = nixpkgs_config_base
        );

        // Write nixpkgs config to a content-addressed file (NIXPKGS_CONFIG expects a file path)
        // Using content hash in filename allows eval cache to track it properly while
        // avoiding race conditions between parallel sessions (same content = same file)
        let config_hash = &compute_string_hash(&nixpkgs_config_nix)[..16];
        let nixpkgs_config_path = paths
            .dotfile
            .join(format!("nixpkgs-config-{}.nix", config_hash));
        std::fs::write(&nixpkgs_config_path, &nixpkgs_config_nix)
            .map_err(|e| miette::miette!("Failed to write nixpkgs config: {}", e))?;
        let nixpkgs_config_path_str = nixpkgs_config_path
            .to_str()
            .ok_or_else(|| miette::miette!("Nixpkgs config path contains invalid UTF-8"))?
            .to_string();

        // Create eval state with flake support and NIXPKGS_CONFIG
        // load_config() loads settings from nix.conf files including access-tokens
        let eval_state = EvalStateBuilder::new(store.clone())
            .to_miette()
            .wrap_err("Failed to create eval state builder")?
            .base_directory(
                paths
                    .root
                    .to_str()
                    .ok_or_else(|| miette::miette!("Root path contains invalid UTF-8"))?,
            )
            .to_miette()
            .wrap_err("Failed to set base directory")?
            .env_override("NIXPKGS_CONFIG", &nixpkgs_config_path_str)
            .to_miette()
            .wrap_err("Failed to set NIXPKGS_CONFIG environment override")?
            .flakes(&flake_settings)
            .to_miette()
            .wrap_err("Failed to configure flakes in eval state")?;
        let mut eval_state = eval_state
            .build()
            .to_miette()
            .wrap_err("Failed to build eval state")?;

        // Enable Nix debugger if requested
        if global_options.nix_debugger {
            eval_state
                .enable_debugger()
                .to_miette()
                .wrap_err("Failed to enable Nix debugger")?;
        }

        // Set up activity logger integration with tracing
        // MUST be initialized after EvalState is created to ensure Nix is initialized
        // The bridge tracks eval activities dynamically - begin_eval/end_eval are called
        // automatically by eval_session() via RAII
        let logger_setup =
            crate::logger::setup_nix_logger().wrap_err("Failed to set up activity logger")?;
        let activity_logger = logger_setup.logger;
        let log_bridge = logger_setup.bridge;

        let cachix_daemon: Arc<tokio::sync::Mutex<Option<crate::cachix_daemon::OwnedDaemon>>> =
            Arc::new(tokio::sync::Mutex::new(None));

        // Create oneshot channel for cleanup completion signaling
        let (cleanup_tx, cleanup_rx) = tokio::sync::oneshot::channel::<()>();
        shutdown.set_cleanup_receiver(cleanup_rx);

        let cachix_activity: Arc<tokio::sync::Mutex<Option<Activity>>> =
            Arc::new(tokio::sync::Mutex::new(None));

        // Spawn cleanup task that waits for shutdown signal via cancellation token.
        let daemon_for_cleanup = cachix_daemon.clone();
        let activity_for_cleanup = cachix_activity.clone();
        let shutdown_for_task = shutdown.clone();
        tokio::spawn(async move {
            // Wait for shutdown signal
            shutdown_for_task.cancellation_token().cancelled().await;

            // Only interrupt Nix operations if this was a user-initiated shutdown (actual signal received).
            // Without this check, normal backend drops would set a global interrupt flag that persists
            // and causes subsequent Nix operations to fail with "interrupted by the user".
            if shutdown_for_task.last_signal().is_some() {
                nix_bindings_util::trigger_interrupt();
            }

            // Cleanup: finalize any queued cachix pushes
            let daemon = {
                let mut guard = daemon_for_cleanup.lock().await;
                guard.take()
            };

            if let Some(daemon) = daemon {
                tracing::info!("Finalizing cachix pushes on shutdown...");
                if let Err(e) = daemon.shutdown(Duration::from_secs(300)).await {
                    tracing::warn!("Error during cachix daemon shutdown: {}", e);
                }
            }

            // Drop the cachix activity to emit Operation::Complete
            let _ = activity_for_cleanup.lock().await.take();

            // Signal cleanup complete (ignore error if receiver dropped)
            let _ = cleanup_tx.send(());
        });

        let backend = Self {
            global_options,
            paths,
            config,
            bootstrap_path,
            nixpkgs_config_path,
            nix_log_bridge: log_bridge,
            eval_cache_pool,
            cache_invalidated: AtomicBool::new(false),
            cachix_manager,
            cachix_daemon: cachix_daemon.clone(),
            cachix_activity: cachix_activity.clone(),
            cached_import_expr: Arc::new(OnceCell::new()),
            cached_args_nix_eval: Arc::new(OnceCell::new()),
            shutdown: shutdown.clone(),
            port_allocator,
            caching_eval_state: OnceCell::new(),
            eval_state: Arc::new(Mutex::new(eval_state)),
            store: Arc::new(store),
            flake_settings,
            fetchers_settings,
            activity_logger,
            _gc_registration: gc_registration,
        };

        Ok(backend)
    }

    /// Create an eval session with activity tracking.
    ///
    /// This method:
    /// 1. Acquires the eval_state mutex
    /// 2. Registers the activity ID for file evaluation logging
    /// 3. Returns an EvalSession that clears the activity ID on drop
    ///
    /// The caller owns the Activity and controls its lifecycle. File evaluations
    /// during this session will be logged to the provided activity.
    fn eval_session(&self, activity: &Activity) -> Result<EvalSession<'_>> {
        // Acquire the eval_state lock
        let guard = self
            .eval_state
            .lock()
            .map_err(|e| miette!("Failed to lock eval state: {}", e))?;

        // begin_eval returns a guard that calls end_eval on drop
        let eval_activity = self.nix_log_bridge.begin_eval(activity.id());

        Ok(EvalSession {
            guard,
            _eval_activity: eval_activity,
        })
    }

    /// Extract embedded bootstrap files to filesystem for Nix to access.
    /// Only writes files whose content has changed to preserve mtimes for direnv.
    fn extract_bootstrap_files(dotfile_dir: &Path) -> Result<PathBuf> {
        use std::io::Write;

        let bootstrap_path = dotfile_dir.join("bootstrap");
        std::fs::create_dir_all(&bootstrap_path)
            .into_diagnostic()
            .wrap_err("Failed to create bootstrap directory")?;

        // Extract all files from embedded bootstrap directory
        for file in BOOTSTRAP_DIR.files() {
            let target_path = bootstrap_path.join(file.path());

            // Create parent directories if needed
            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent)
                    .into_diagnostic()
                    .wrap_err("Failed to create parent directories")?;
            }

            // Skip writing if existing file already has the same content
            if let Ok(existing) = std::fs::read(&target_path) {
                if existing == file.contents() {
                    continue;
                }
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

    /// Get absolute path to a file in the bootstrap directory
    pub fn bootstrap_file(&self, relative_path: &str) -> PathBuf {
        self.bootstrap_path.join(relative_path)
    }

    /// Evaluate `import default.nix args` with primops injected.
    ///
    /// This creates the `allocatePort` primop as a Nix Value and merges it into
    /// the args attrset before calling the import function. This ensures Nix modules
    /// can call `primops.allocatePort` to allocate ports during evaluation.
    ///
    /// The approach:
    /// 1. Evaluate `import /path/to/default.nix` to get the function
    /// 2. Evaluate the serialized args string to get the base args attrset
    /// 3. Create the `allocatePort` primop using PrimOp::new
    /// 4. Build `{ primops = { allocatePort = <primop>; }; }` as a Value
    /// 5. Merge: `baseArgs // { primops = ...; }`
    /// 6. Apply: `(import default.nix) mergedArgs`
    fn eval_import_with_primops(
        &self,
        eval_state: &mut EvalState,
    ) -> Result<nix_bindings_expr::value::Value> {
        let args_nix = self
            .cached_args_nix_eval
            .get()
            .expect("assemble() must be called first");
        let base = self.paths.root.to_str().unwrap();

        // 1. Get the import function
        let import_path = self.bootstrap_file("default.nix");
        // Escape the Nix path literal
        let import_nix_path = ser_nix::to_string(&ser_nix::NixPathBuf::from(import_path))
            .into_diagnostic()
            .wrap_err("Failed to serialize import path")?;
        let import_fn = eval_state
            .eval_from_string(&format!("import ({import_nix_path})"), base)
            .to_miette()
            .wrap_err("Failed to evaluate import expression")?;

        // 2. Evaluate the serialized args to get the base attrset
        let base_args = eval_state
            .eval_from_string(args_nix, base)
            .to_miette()
            .wrap_err("Failed to evaluate NixArgs")?;

        // 3. Build primops attrset based on whether port allocation is enabled
        // When disabled, we still need to pass primops = {} for module compatibility
        tracing::debug!(
            "eval_import_with_primops: is_enabled={}, is_strict={}",
            self.port_allocator.is_enabled(),
            self.port_allocator.is_strict()
        );
        let primops_attrset = if self.port_allocator.is_enabled() {
            // Create the allocatePort primop: processName -> portName -> basePort -> allocatedPort
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
            // Empty primops attrset when port allocation is disabled
            eval_state
                .new_value_attrs(vec![])
                .to_miette()
                .wrap_err("Failed to create empty primops attrset")?
        };

        // 5. Build override: { primops = { allocatePort = <primop>; }; }
        let override_attrs = eval_state
            .new_value_attrs(vec![("primops".to_string(), primops_attrset)])
            .to_miette()
            .wrap_err("Failed to create override attrset")?;

        // 6. Merge base args with primops override: baseArgs // { primops = ...; }
        let merge_fn = eval_state
            .eval_from_string("a: b: a // b", "<primop-injection>")
            .to_miette()
            .wrap_err("Failed to create merge function")?;
        let final_args = eval_state
            .call_multi(&merge_fn, &[base_args, override_attrs])
            .to_miette()
            .wrap_err("Failed to merge args with primops")?;

        // 7. Apply: (import default.nix) finalArgs
        tracing::debug!("eval_import_with_primops: calling import function with merged args");
        eval_state
            .call(import_fn, final_args)
            .to_miette()
            .wrap_err("Failed to evaluate devenv configuration")
    }

    /// Apply GlobalOptions settings to the Nix environment
    ///
    /// This must be called BEFORE opening the Store or creating EvalState,
    /// as these settings affect how Nix initializes.
    ///
    /// THREAD SAFETY: The underlying Nix settings use global mutable state.
    /// This should only be called during single-threaded initialization.
    fn apply_global_options(global_options: &GlobalOptions) -> Result<()> {
        // Disable flake evaluation cache to avoid stale cache issues
        settings::set("eval-cache", "false")
            .to_miette()
            .wrap_err("Failed to disable eval-cache")?;

        // Always allow substitutes even if they could be built locally
        settings::set("always-allow-substitutes", "true")
            .to_miette()
            .wrap_err("Failed to set always-allow-substitutes")?;

        // Improve parallelism during downloads
        settings::set("http-connections", "100")
            .to_miette()
            .wrap_err("Failed to set http-connections")?;

        // offline mode: disable substituters and set file transfer timeouts
        if global_options.offline {
            settings::set("substituters", "")
                .to_miette()
                .wrap_err("Failed to set offline mode (substituters)")?;

            // Set connection timeout for offline mode (fail fast)
            settings::set("connect-timeout", "1")
                .to_miette()
                .wrap_err("Failed to set connect-timeout for offline mode")?;
        }

        // max_jobs: set maximum concurrent builds
        if global_options.max_jobs > 0 {
            settings::set("max-jobs", &global_options.max_jobs.to_string())
                .to_miette()
                .wrap_err("Failed to set max-jobs")?;
        }

        // cores: set CPU cores available per build
        if global_options.cores > 0 {
            settings::set("cores", &global_options.cores.to_string())
                .to_miette()
                .wrap_err("Failed to set cores")?;
        }

        // system: override the build system architecture
        // Skip default/placeholder values to avoid overriding with nonsense
        if !global_options.system.is_empty()
            && global_options.system != "unknown architecture-unknown OS"
        {
            settings::set("system", &global_options.system)
                .to_miette()
                .wrap_err("Failed to set system")?;
        }

        // impure: allow impure evaluation (relaxes hermeticity)
        if global_options.impure {
            settings::set("impure", "1")
                .to_miette()
                .wrap_err("Failed to set impure mode")?;
        } else {
            // Enable pure evaluation by default when not in impure mode
            // This restricts file system and network access during evaluation
            settings::set("pure-eval", "true")
                .to_miette()
                .wrap_err("Failed to set pure-eval mode")?;
            // Allow local filesystem paths while maintaining other purity guarantees
            // This enables evaluating local devenv.nix files without copying entire repo to store
            settings::set("pure-eval-allow-local-paths", "true")
                .to_miette()
                .wrap_err("Failed to set pure-eval-allow-local-paths")?;
        }

        // Enable registry lookups for flake input resolution
        // Required for flakes with transitive inputs using indirect references (e.g. flake:nixpkgs)
        settings::set("use-registries", "true")
            .to_miette()
            .wrap_err("Failed to set use-registries")?;

        // nix_option: apply custom Nix configuration pairs
        // These are passed as pairs: ["key1", "value1", "key2", "value2", ...]
        for pair in global_options.nix_option.chunks_exact(2) {
            let key = &pair[0];
            let value = &pair[1];
            settings::set(key, value)
                .to_miette()
                .wrap_err(format!("Failed to set nix option: {key} = {value}"))?;
        }

        Ok(())
    }

    /// Get cachix configuration from devenv.nix via eval cache.
    ///
    /// Returns the cachix configuration if enabled, None otherwise.
    async fn get_cachix_config(&self) -> Result<Option<CachixCacheInfo>> {
        if self.global_options.offline {
            return Ok(None);
        }

        let caching_state = self
            .caching_eval_state
            .get()
            .expect("assemble() must be called first");

        let cache_key = caching_state.cache_key("config.cachix");
        let activity = Activity::evaluate("Checking cachix config")
            .level(ActivityLevel::Debug)
            .start();

        let (json_str, _cache_hit) = async {
            caching_state
                .cached_eval()
                .eval(&cache_key, &activity, || async {
                    self.eval_attr_uncached("config.cachix", "config.cachix", &activity)
                })
                .await
        }
        .in_activity(&activity)
        .await
        .map_err(cache_error_to_miette)?;

        let cachix_config: CachixConfig = match serde_json::from_str(&json_str) {
            Ok(config) => config,
            Err(e) => {
                tracing::warn!("Failed to parse cachix config: {}", e);
                return Ok(None);
            }
        };

        if !cachix_config.enable {
            return Ok(None);
        }

        // Load known keys from trusted keys file
        let trusted_keys_path = &self.cachix_manager.paths.trusted_keys;
        let known_keys = if trusted_keys_path.exists() {
            std::fs::read_to_string(trusted_keys_path)
                .ok()
                .and_then(|content| serde_json::from_str(&content).ok())
                .unwrap_or_default()
        } else {
            Default::default()
        };

        Ok(Some(CachixCacheInfo {
            caches: cachix_config.caches,
            known_keys,
            binary: cachix_config.binary,
        }))
    }

    /// Apply cachix substituters and trusted keys to the Nix store.
    ///
    /// THREAD SAFETY: These Nix FFI calls modify global state and are not thread-safe.
    /// Should only be called during initialization.
    async fn apply_cachix_substituters(&self, cachix_config: &CachixCacheInfo) -> Result<()> {
        match self.cachix_manager.get_nix_settings(cachix_config).await {
            Ok(settings) => {
                let mut store = (*self.store).clone();

                if let Some(extra_substituters) = settings.get("extra-substituters") {
                    for substituter in extra_substituters.split_whitespace() {
                        if let Err(e) = store.add_substituter(substituter).to_miette() {
                            tracing::warn!("Failed to add substituter {}: {}", substituter, e);
                        } else {
                            tracing::debug!("Added substituter: {}", substituter);
                        }
                    }
                }

                if let Some(extra_trusted_keys) = settings.get("extra-trusted-public-keys") {
                    let keys: Vec<&str> = extra_trusted_keys.split_whitespace().collect();
                    if !keys.is_empty() {
                        tracing::debug!("Adding {} trusted public keys via store FFI", keys.len());
                        if let Err(e) = store.add_trusted_public_keys(&keys).to_miette() {
                            tracing::warn!("Failed to add trusted public keys: {}", e);
                        } else {
                            tracing::debug!("Added {} trusted public keys", keys.len());
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to get cachix nix settings: {}", e);
            }
        }

        Ok(())
    }

    /// Validate and ensure lock file is up-to-date with the current devenv configuration
    ///
    /// This method:
    /// 1. If lock file doesn't exist, creates it by calling update()
    /// 2. If lock file exists, verifies it's in sync with current configuration
    /// 3. If out of sync, automatically refreshes the locks by calling update()
    ///
    /// This matches the behavior of flakes - locks are automatically created/updated as needed.
    async fn validate_lock_file(&self) -> Result<()> {
        use crate::{create_flake_inputs, load_lock_file};

        let fetch_settings = &self.fetchers_settings;
        let flake_settings = &self.flake_settings;
        let lock_file_path = self.paths.root.join("devenv.lock");

        // If lock file doesn't exist, create it
        if !lock_file_path.exists() {
            return self.update(&None).await;
        }

        // Load existing lock file
        let old_lock = load_lock_file(fetch_settings, &lock_file_path)
            .to_miette()
            .wrap_err("Failed to load lock file")?;

        let old_lock = match old_lock {
            Some(lock) => lock,
            None => {
                // Lock file is invalid/empty - regenerate it
                return self.update(&None).await;
            }
        };

        // Convert devenv inputs to flake inputs
        let flake_inputs = create_flake_inputs(fetch_settings, flake_settings, &self.config)
            .to_miette()
            .wrap_err("Failed to create flake inputs")?;

        // Nix's resolveRelativePath uses parent() on the source_path to get the directory.
        // Since it expects a file path (like flake.nix), we append devenv.nix so parent() returns our root.
        let source_path = self.paths.root.join("devenv.nix");
        let source_path_str = source_path
            .to_str()
            .ok_or_else(|| miette!("Source path contains invalid UTF-8"))?;

        // Create a locker in Virtual mode so unlocked local inputs don't fail validation.
        // We compare the computed lock against the existing one to detect drift.
        let locker = InputsLocker::new(flake_settings)
            .with_inputs(flake_inputs)
            .source_path(source_path_str)
            .old_lock_file(&old_lock)
            .mode(LockMode::Virtual)
            .use_registries(true);

        let activity = Activity::evaluate("Validating lock")
            .level(ActivityLevel::Debug)
            .start();

        let lock_result = {
            let eval_state = self.eval_session(&activity)?;
            locker.lock(fetch_settings, &eval_state)
        };

        drop(activity);

        match lock_result {
            Ok(new_lock) => {
                if new_lock.has_changes(&old_lock).to_miette()? {
                    tracing::debug!("Lock validation found changes, updating lock");
                    return self.update(&None).await;
                }
            }
            Err(e) => {
                tracing::debug!("Lock validation failed: {e}, updating lock");
                return self.update(&None).await;
            }
        }

        Ok(())
    }

    async fn init_cachix_daemon(&self, push_cache: &str, binary: &Path) -> Result<()> {
        tracing::debug!(binary = %binary.display(), "Starting cachix daemon");

        let socket_path = self
            .cachix_manager
            .paths
            .daemon_socket
            .clone()
            .unwrap_or_else(|| {
                std::env::temp_dir().join(format!("cachix-daemon-{}.sock", std::process::id()))
            });

        let spawn_config = crate::cachix_daemon::DaemonSpawnConfig {
            cache_name: push_cache.to_string(),
            socket_path,
            binary: binary.to_path_buf(),
            dry_run: false,
        };

        let activity = Activity::operation(format!("Pushing to {}", push_cache)).start();

        match crate::cachix_daemon::OwnedDaemon::spawn(
            spawn_config,
            crate::cachix_daemon::ConnectionParams::default(),
            Some(activity.clone()),
        )
        .await
        {
            Ok(daemon) => {
                let mut handle = self.cachix_daemon.lock().await;
                *handle = Some(daemon);
                *self.cachix_activity.lock().await = Some(activity);
                tracing::info!(push_cache, "Cachix daemon started");
            }
            Err(e) => {
                activity.fail();
                tracing::warn!("Failed to start cachix daemon: {}", e);
            }
        }
        Ok(())
    }

    /// Queue store paths for real-time pushing to cachix (fire-and-forget)
    ///
    /// This queues paths immediately as they're realized without blocking.
    /// The background daemon will process them asynchronously in parallel with the build.
    ///
    /// Push completion is automatically handled when the backend is dropped.
    ///
    /// This is a fire-and-forget operation - it does not wait for push completion.
    async fn queue_realized_paths(&self, store_paths: &[PathBuf]) -> Result<()> {
        if store_paths.is_empty() {
            return Ok(());
        }

        let path_strings: Vec<String> = store_paths
            .iter()
            .filter_map(|p| p.to_str().map(|s| s.to_string()))
            .collect();

        if path_strings.is_empty() {
            return Ok(());
        }

        // Acquire lock once and queue all paths in a single operation
        let daemon_guard = self.cachix_daemon.lock().await;
        if let Some(daemon) = daemon_guard.as_ref()
            && let Err(e) = daemon.queue_paths(path_strings).await
        {
            tracing::warn!("Failed to queue paths to cachix: {}", e);
        }

        Ok(())
    }

    /// Wait for all queued cachix pushes to complete and report results
    /// This does NOT queue any new paths - it only waits for previously queued paths.
    /// Called at the end of operations to ensure all pushes complete before returning.
    ///
    /// This is especially important in async contexts where the Drop handler cannot block_on.
    pub async fn finalize_cachix_push(&self) -> Result<()> {
        // Check if daemon is active
        let has_daemon = self.cachix_daemon.lock().await.as_ref().is_some();

        if !has_daemon {
            return Ok(());
        }

        tracing::debug!("Waiting for cachix push completion");

        // Wait for completion
        let daemon = self.cachix_daemon.lock().await;

        if let Some(daemon_ref) = daemon.as_ref() {
            match daemon_ref
                .wait_for_completion(Duration::from_secs(300))
                .await
            {
                Ok(metrics) => {
                    tracing::info!("{}", metrics.summary());

                    if metrics.failed > 0 {
                        let mut activity_guard = self.cachix_activity.lock().await;
                        if let Some(ref activity) = *activity_guard {
                            activity.fail();
                            let failed_reasons = metrics.failed_with_reasons.lock().await;
                            for (path, reason) in failed_reasons.iter() {
                                activity.error(format!("{}: {}", path, reason));
                            }
                        }
                        // Drop to emit the failed Complete event
                        let _ = activity_guard.take();
                    } else {
                        // Drop to emit the successful Complete event
                        let _ = self.cachix_activity.lock().await.take();
                    }
                }
                Err(e) => {
                    tracing::warn!("Timeout waiting for cachix push completion: {}", e);
                    let mut activity_guard = self.cachix_activity.lock().await;
                    if let Some(ref activity) = *activity_guard {
                        activity.fail();
                    }
                    let _ = activity_guard.take();
                }
            }
        }

        Ok(())
    }
}

#[async_trait(?Send)]
impl NixBackend for NixRustBackend {
    async fn lock_fingerprint(&self) -> Result<String> {
        let lock_file_path = self.paths.root.join("devenv.lock");
        let lock_file = crate::load_lock_file(&self.fetchers_settings, &lock_file_path)
            .to_miette()
            .wrap_err("Failed to load lock file for fingerprint computation")?;
        crate::compute_lock_fingerprint(lock_file.as_ref(), &self.store)
            .to_miette()
            .wrap_err("Failed to compute lock fingerprint")
    }

    async fn assemble(&self, args: &NixArgs<'_>) -> Result<()> {
        // Initialize caching eval state if not already set
        if self.caching_eval_state.get().is_none() {
            let args_nix = ser_nix::to_string(args).unwrap_or_else(|_| "{}".to_string());
            let cache_key_args = format!(
                "{}:port_allocation={}:strict_ports={}",
                args_nix,
                self.port_allocator.is_enabled(),
                self.port_allocator.is_strict()
            );

            // Unquote special Nix expressions that should be evaluated
            let args_nix_eval =
                args_nix.replace("\"builtins.currentSystem\"", "builtins.currentSystem");

            let import_path = self.bootstrap_file("default.nix");
            let import_nix_path = ser_nix::to_string(&ser_nix::NixPathBuf::from(import_path))
                .into_diagnostic()
                .wrap_err("Failed to serialize import path")?;
            let import_expr = format!("(import ({import_nix_path}) {args_nix_eval})",);

            self.cached_import_expr.set(import_expr).ok();
            self.cached_args_nix_eval.set(args_nix_eval).ok();

            // Create resource manager for port allocation tracking across cache hits
            let resource_manager = Arc::new(ResourceManager::new(self.port_allocator.clone()));

            // Create CachedEval wrapper
            let cached_eval = if let Some(ref pool_cell) = self.eval_cache_pool {
                if let Some(pool) = pool_cell.get() {
                    let config = CachingConfig {
                        force_refresh: self.global_options.refresh_eval_cache,
                        // NIXPKGS_CONFIG is already tracked via NixArgs.nixpkgs_config
                        excluded_envs: vec!["NIXPKGS_CONFIG".to_string()],
                        // The nixpkgs config file content is already reflected in the cache key
                        excluded_paths: vec![self.nixpkgs_config_path.clone()],
                        ..Default::default()
                    };
                    let service = CachingEvalService::with_config(pool.clone(), config.clone());
                    tracing::debug!(?config, "Eval caching enabled from framework pool");
                    CachedEval::with_cache(service, self.nix_log_bridge.clone(), config)
                        .with_resource_manager(resource_manager)
                } else {
                    tracing::debug!("Eval caching disabled (pool not ready)");
                    CachedEval::without_cache(self.nix_log_bridge.clone())
                }
            } else {
                tracing::debug!("Eval caching disabled (no pool configured)");
                CachedEval::without_cache(self.nix_log_bridge.clone())
            };

            // Create unified CachingEvalState wrapper
            let caching_eval_state =
                CachingEvalState::new(self.eval_state.clone(), cached_eval, cache_key_args);
            self.caching_eval_state.set(caching_eval_state).ok();
        }

        // Validate lock file once during assembly
        // This ensures all subsequent evaluations have a valid lock to work with
        self.validate_lock_file().await?;

        // Configure cachix substituters and start daemon if push is configured
        if let Some(cachix_config) = self.get_cachix_config().await? {
            self.apply_cachix_substituters(&cachix_config).await?;
            if let Some(ref push_cache) = cachix_config.caches.push {
                self.init_cachix_daemon(push_cache, &cachix_config.binary)
                    .await?;
            }
        }

        Ok(())
    }

    async fn dev_env(&self, json: bool, gc_root: &Path) -> Result<DevEnvOutput> {
        // Evaluate the devenv shell environment from default.nix
        // This replaces: nix print-dev-env --profile gc_root [--json]

        // Note: Lock file validation is done in assemble(), which must be called first

        let caching_state = self
            .caching_eval_state
            .get()
            .expect("assemble() must be called first");

        // Create cache key for shell paths
        let cache_key = caching_state.cache_key("shell");

        // Check if cache was invalidated (for hot-reload)
        let cache_invalidated = self.cache_invalidated.swap(false, Ordering::AcqRel);
        if cache_invalidated {
            tracing::debug!("Cache bypassed due to invalidation (hot-reload)");
        }

        // Try to get cached paths and verify they still exist
        // Note: dev_env requires explicit path validation because store paths can be GC'd
        let cached_paths: Option<CachedShellPaths> = if cache_invalidated {
            // Skip cache lookup entirely when invalidated
            None
        } else if let Some(service) = caching_state.cached_eval().service() {
            match service.get_cached(&cache_key).await {
                Ok(Some(cached)) => {
                    match serde_json::from_str::<CachedShellPaths>(&cached.json_output) {
                        Ok(paths) => {
                            // Verify both paths still exist (may have been garbage collected)
                            let drv_exists = std::path::Path::new(&paths.drv_path).exists();
                            let out_exists = std::path::Path::new(&paths.out_path).exists();
                            if drv_exists && out_exists {
                                // Replay resource allocations (ports) for this cached eval
                                match caching_state
                                    .cached_eval()
                                    .try_replay_resources(cached.eval_id)
                                    .await
                                {
                                    Ok(()) => {
                                        tracing::debug!(
                                            drv_path = %paths.drv_path,
                                            out_path = %paths.out_path,
                                            "Eval cache hit for shell"
                                        );
                                        Some(paths)
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            error = %e,
                                            "Resource replay failed for shell cache hit, re-evaluating"
                                        );
                                        caching_state.cached_eval().clear_resources();
                                        None
                                    }
                                }
                            } else {
                                tracing::debug!(
                                    drv_path = %paths.drv_path,
                                    out_path = %paths.out_path,
                                    drv_exists = drv_exists,
                                    out_exists = out_exists,
                                    "Cached paths no longer exist (garbage collected?)"
                                );
                                None
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "Failed to parse cached shell paths");
                            None
                        }
                    }
                }
                Ok(None) => {
                    tracing::trace!("Eval cache miss for shell");
                    None
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Error checking eval cache for shell");
                    None
                }
            }
        } else {
            None
        };

        let activity = Activity::evaluate("Evaluating shell")
            .level(ActivityLevel::Info)
            .start();

        let (drv_path_str, out_path_str, env_path, cache_hit) = if let Some(paths) = cached_paths {
            // Cache hit - skip evaluation entirely
            activity.cached();
            (paths.drv_path, paths.out_path, paths.env_path, true)
        } else {
            // Cache miss or invalid paths - do full evaluation
            let result = async {
                caching_state
                    .cached_eval()
                    .eval_typed::<CachedShellPaths, _, _>(&cache_key, &activity, || async {
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
                    return Err(cache_error_to_miette(e));
                }
            }
        };

        // Parse store path and create GC root
        let mut store = (*self.store).clone();
        let store_path = store
            .parse_store_path(&out_path_str)
            .to_miette()
            .wrap_err("Failed to parse output store path")?;

        // Remove existing symlink right before creating new one to minimize race window
        if gc_root.symlink_metadata().is_ok() {
            std::fs::remove_file(gc_root)
                .map_err(|e| miette!("Failed to remove existing GC root: {}", e))?;
        }
        store
            .add_perm_root(&store_path, gc_root)
            .to_miette()
            .wrap_err("Failed to create GC root")?;

        // Queue realized path for real-time pushing (only on fresh build)
        if !cache_hit {
            self.queue_realized_paths(&[PathBuf::from(&out_path_str)])
                .await?;
        }

        // Try to use cached env JSON directly (fast path), otherwise fall back to FFI
        let output_str = if let Some(ref env_path) = env_path {
            if std::path::Path::new(env_path).exists() {
                // Fast path: read cached -env JSON directly, skip expensive FFI
                tracing::debug!(env_path = %env_path, "Using cached env JSON (skipping FFI)");
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
                tracing::debug!(env_path = %env_path, "Cached env path no longer exists, falling back to FFI");
                self.build_dev_environment(&mut store, &drv_path_str, json)?
            }
        } else {
            // No cached env path, use FFI
            tracing::debug!("No cached env path, using FFI");
            self.build_dev_environment(&mut store, &drv_path_str, json)?
        };

        // Get file inputs from cache for direnv to watch
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

    async fn repl(&self) -> Result<()> {
        // Initialize the Nix command library (REPL support)
        nix_cmd::init()
            .to_miette()
            .wrap_err("Failed to initialize Nix command library")?;

        // Reset the logger to restore normal stderr output for the REPL
        self.activity_logger.reset();

        // Print any stored errors (captured during evaluation)
        for error in self.nix_log_bridge.take_pre_repl_errors() {
            eprintln!("{}", error);
        }

        // Lock the eval_state for REPL access
        let activity = Activity::evaluate("Evaluating Nix")
            .level(ActivityLevel::Info)
            .start();
        let mut eval_state = self.eval_session(&activity)?;

        // Check if there's a pending debugger session from a previous error
        // If so, run the debugger REPL which has the error context
        let status = if nix_cmd::debugger_is_pending() {
            nix_cmd::debugger_run_pending(&mut eval_state)
                .to_miette()
                .wrap_err("Debugger REPL failed")?
        } else {
            // Load default.nix with primops into the REPL scope
            let devenv_attrs = self.eval_import_with_primops(&mut eval_state)?;

            // Create a ValMap to inject variables into the REPL scope
            let mut env = nix_cmd::ValMap::new()
                .to_miette()
                .wrap_err("Failed to create REPL environment")?;

            // Inject the devenv configuration as "devenv" variable in REPL
            env.insert("devenv", &devenv_attrs)
                .to_miette()
                .wrap_err("Failed to inject devenv into REPL scope")?;

            // Extract and inject pkgs for convenience
            let pkgs = eval_state
                .require_attrs_select(&devenv_attrs, "pkgs")
                .to_miette()
                .wrap_err("Failed to get pkgs attribute from devenv")?;
            env.insert("pkgs", &pkgs)
                .to_miette()
                .wrap_err("Failed to inject pkgs into REPL scope")?;

            // Run the interactive REPL with pre-populated environment
            nix_cmd::run_repl_simple(&mut eval_state, Some(&mut env))
                .to_miette()
                .wrap_err("REPL failed")?
        };

        // Handle the exit status
        match status {
            ReplExitStatus::QuitAll => {
                // User exited with :quit - exit the program
                std::process::exit(0);
            }
            ReplExitStatus::Continue => {
                // User exited with :continue - return normally
                Ok(())
            }
        }
    }

    async fn build(
        &self,
        attributes: &[&str],
        _options: Option<Options>,
        gc_root: Option<&Path>,
    ) -> Result<Vec<PathBuf>> {
        // Build derivations and return output paths with transparent caching
        // Caches output store paths per attribute to skip redundant builds

        if attributes.is_empty() {
            return Ok(Vec::new());
        }

        // Note: Lock file validation is done in assemble(), which must be called first

        let caching_state = self
            .caching_eval_state
            .get()
            .expect("assemble() must be called first");

        let mut output_paths = Vec::new();

        for attr_path in attributes {
            // Cache key includes ":build" suffix to distinguish from eval cache
            let cache_key = caching_state.cache_key(&format!("{}:build", attr_path));

            // Check cache for existing build output path
            let cached_path: Option<String> = if let Some(service) =
                caching_state.cached_eval().service()
            {
                match service.get_cached(&cache_key).await {
                    Ok(Some(cached)) => {
                        match serde_json::from_str::<String>(&cached.json_output) {
                            Ok(path_str) => {
                                // Verify path still exists (may have been garbage collected)
                                if std::path::Path::new(&path_str).exists() {
                                    tracing::debug!(
                                        attr_path = attr_path,
                                        path = %path_str,
                                        "Build cache hit"
                                    );
                                    Some(path_str)
                                } else {
                                    tracing::debug!(
                                        attr_path = attr_path,
                                        path = %path_str,
                                        "Cached build path no longer exists (garbage collected?)"
                                    );
                                    None
                                }
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "Failed to parse cached build path");
                                None
                            }
                        }
                    }
                    Ok(None) => {
                        tracing::trace!(attr_path = attr_path, "Build cache miss");
                        None
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Error checking build cache");
                        None
                    }
                }
            } else {
                None
            };

            let activity = Activity::evaluate(format!("Evaluating {}", attr_path))
                .level(ActivityLevel::Info)
                .start();

            let (path_str, cache_hit) = if let Some(path) = cached_path {
                // Cache hit - skip evaluation entirely
                activity.cached();
                (path, true)
            } else {
                // Cache miss or invalid path - do full build
                let attr_path = attr_path.to_string();
                let (path, _) = async {
                    caching_state
                        .cached_eval()
                        .eval_typed::<String, _, _>(&cache_key, &activity, || async {
                            self.build_attr_uncached(&attr_path, &activity)
                        })
                        .await
                }
                .in_activity(&activity)
                .await
                .map_err(cache_error_to_miette)?;

                (path, false)
            };

            let path = PathBuf::from(&path_str);

            // Add GC root if requested, named after the attribute
            if let Some(gc_root_base) = gc_root {
                let mut store = (*self.store).clone();
                let store_path = store
                    .parse_store_path(&path_str)
                    .to_miette()
                    .wrap_err("Failed to parse store path")?;

                // Sanitize attribute path for use as filename (replace dots with dashes)
                let sanitized_attr = attr_path.replace('.', "-");
                let attr_gc_root = gc_root_base.with_file_name(format!(
                    "{}-{}",
                    gc_root_base
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy(),
                    sanitized_attr
                ));

                // Remove existing symlink right before creating new one
                if attr_gc_root.symlink_metadata().is_ok() {
                    std::fs::remove_file(&attr_gc_root)
                        .map_err(|e| miette!("Failed to remove existing GC root: {}", e))?;
                }

                store
                    .add_perm_root(&store_path, &attr_gc_root)
                    .to_miette()
                    .wrap_err("Failed to add GC root")?;
            }

            output_paths.push(path.clone());

            // Queue realized path for real-time pushing (only on fresh build)
            if !cache_hit {
                self.queue_realized_paths(&[path]).await?;
            }
        }

        Ok(output_paths)
    }

    async fn eval(&self, attributes: &[&str]) -> Result<String> {
        // Evaluate Nix expressions and return JSON
        // Evaluates attributes from default.nix with transparent caching
        // Note: Lock file validation is done in assemble(), which must be called first

        let caching_state = self
            .caching_eval_state
            .get()
            .expect("assemble() must be called first");

        let mut results = Vec::new();

        for attr_path in attributes {
            // Parse attribute path - remove leading ".#" if present
            let clean_path = attr_path.trim_start_matches(".#");

            let cache_key = caching_state.cache_key(clean_path);
            let activity = Activity::evaluate("Evaluating Nix")
                .level(ActivityLevel::Info)
                .start();

            let attr_path_owned = attr_path.to_string();
            let clean_path_owned = clean_path.to_string();

            let (json_str, _cache_hit) = async {
                caching_state
                    .cached_eval()
                    .eval(&cache_key, &activity, || async {
                        self.eval_attr_uncached(&attr_path_owned, &clean_path_owned, &activity)
                    })
                    .await
            }
            .in_activity(&activity)
            .await
            .map_err(cache_error_to_miette)?;

            results.push(json_str);
        }

        // If multiple attributes, wrap in array; if single, return as-is
        if results.len() == 1 {
            Ok(results.into_iter().next().unwrap())
        } else {
            Ok(format!("[{results_str}]", results_str = results.join(",")))
        }
    }

    async fn update(&self, input_name: &Option<String>) -> Result<()> {
        use crate::{create_flake_inputs, load_lock_file, write_lock_file};

        // Use settings created during backend initialization - ensures consistency
        let fetch_settings = &self.fetchers_settings;
        let flake_settings = &self.flake_settings;

        // Convert devenv inputs to flake inputs using Config and base_dir directly
        let flake_inputs = create_flake_inputs(fetch_settings, flake_settings, &self.config)
            .to_miette()
            .wrap_err("Failed to create flake inputs")?;

        // Determine lock file path
        let lock_file_path = self.paths.root.join("devenv.lock");

        // Load existing lock file
        let old_lock = load_lock_file(fetch_settings, &lock_file_path)
            .to_miette()
            .wrap_err("Failed to load lock file: {}")?;

        // Base directory for parsing override inputs
        let base_dir_str = self
            .paths
            .root
            .to_str()
            .ok_or_else(|| miette!("Root path contains invalid UTF-8"))?;

        // Nix's resolveRelativePath uses parent() on the source_path to get the directory.
        // Since it expects a file path (like flake.nix), we append devenv.nix so parent() returns our root.
        let source_path = self.paths.root.join("devenv.nix");
        let source_path_str = source_path
            .to_str()
            .ok_or_else(|| miette!("Source path contains invalid UTF-8"))?;

        let mut locker = InputsLocker::new(flake_settings)
            .with_inputs(flake_inputs)
            .source_path(source_path_str)
            .mode(LockMode::Virtual)
            .use_registries(true);

        // Set the old lock file if provided
        if let Some(lock) = &old_lock {
            locker = locker.old_lock_file(lock);
        }

        // Mark inputs for update
        if let Some(name) = input_name {
            locker = locker.update_input(name);
        } else {
            locker = locker.update_all();
        }

        // Apply input overrides from global_options
        // Note: overrides must live until after lock() is called
        let overrides: Vec<(String, FlakeReference)> =
            if !self.global_options.override_input.is_empty() {
                let mut parse_flags = FlakeReferenceParseFlags::new(flake_settings).to_miette()?;
                parse_flags.set_base_directory(base_dir_str).to_miette()?;

                self.global_options
                    .override_input
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
                    .collect::<anyhow::Result<Vec<_>>>()
                    .to_miette()
                    .wrap_err("Failed to parse input overrides")?
            } else {
                Vec::new()
            };

        if !overrides.is_empty() {
            locker = locker.overrides(overrides.iter().map(|(name, ref_)| (name.clone(), ref_)));
        }

        // Get eval state from mutex only when needed for locking
        // This ensures we reuse the same eval state across calls
        let activity = Activity::evaluate("Updating inputs")
            .level(ActivityLevel::Info)
            .start();
        let eval_state = self.eval_session(&activity)?;

        // Lock the inputs - pass eval_state by reference to avoid cloning
        let lock_file = locker
            .lock(fetch_settings, &eval_state)
            .to_miette()
            .wrap_err("Failed to lock inputs")?;

        // Write the updated lock file
        write_lock_file(&lock_file, &lock_file_path)
            .to_miette()
            .wrap_err("Failed to write lock file")?;

        Ok(())
    }

    async fn metadata(&self) -> Result<String> {
        // Get metadata: list inputs from lock file and evaluate config.info
        use crate::load_lock_file;

        // Validate lock file before reading metadata
        self.validate_lock_file().await?;

        // PART 1: Format inputs from lock file using FFI iterator
        let lock_file_path = self.paths.root.join("devenv.lock");
        let inputs_section = if lock_file_path.exists() {
            let lock = load_lock_file(&self.fetchers_settings, &lock_file_path)
                .to_miette()
                .wrap_err("Failed to load lock file")?;

            if let Some(lock_file) = lock {
                Self::format_lock_inputs(&lock_file)?
            } else {
                "Inputs:\n  (no lock file)".to_string()
            }
        } else {
            "Inputs:\n  (no lock file)".to_string()
        };

        // PART 2: Evaluate config.info from default.nix (with transparent caching)
        // Use self.eval() which handles caching automatically
        let info_str = match self.eval(&["config.info"]).await {
            Ok(json_str) => {
                // config.info is a string, so the JSON is a quoted string
                // Parse it to extract the actual string value
                serde_json::from_str::<String>(&json_str).unwrap_or_default()
            }
            Err(_) => {
                // config.info doesn't exist or failed to evaluate - that's OK
                String::new()
            }
        };

        // Combine outputs to match original devenv format
        if info_str.is_empty() {
            Ok(inputs_section)
        } else {
            Ok(format!("{inputs_section}\n\n{info_str}"))
        }
    }

    async fn search(&self, name: &str, _options: Option<Options>) -> Result<SearchResults> {
        // Search through pkgs from bootstrap/default.nix for packages matching the query
        // Uses the nix search C API which handles recurseForDerivations logic
        // Respects overlays, locked versions, and devenv configuration

        // Validate lock file before searching
        self.validate_lock_file().await?;

        let activity = Activity::evaluate("Searching packages")
            .level(ActivityLevel::Info)
            .start();
        let mut eval_state = self.eval_session(&activity)?;

        // Import default.nix with primops to get the configured pkgs
        let devenv = self.eval_import_with_primops(&mut eval_state)?;

        // Extract the pkgs attribute from the devenv output
        let pkgs = self.enriched(
            eval_state.require_attrs_select(&devenv, "pkgs"),
            "Failed to get pkgs attribute from devenv configuration",
        )?;

        // Create EvalCache for efficient lazy traversal with optional SQLite caching
        let cache = EvalCache::new(&mut eval_state, &pkgs, None)
            .to_miette()
            .wrap_err("Failed to create eval cache for pkgs")?;

        let cursor = cache
            .root()
            .to_miette()
            .wrap_err("Failed to get root cursor from eval cache")?;

        // Configure search with the query pattern (case-insensitive)
        let mut params = SearchParams::new()
            .to_miette()
            .wrap_err("Failed to create search params")?;

        params
            .add_regex(name)
            .to_miette()
            .wrap_err("Failed to add search regex")?;

        // Collect results using the search API
        let mut results: SearchResults = BTreeMap::new();
        let max_results = 100;

        search(&cursor, Some(&params), |result: SearchResult| {
            if results.len() >= max_results {
                return false; // Stop searching
            }

            results.insert(
                result.attr_path,
                PackageSearchResult {
                    pname: result.name,
                    version: result.version,
                    description: result.description,
                },
            );
            true // Continue searching
        })
        .to_miette()
        .wrap_err("Search failed")?;

        Ok(results)
    }

    async fn gc(&self, paths: Vec<PathBuf>) -> Result<(u64, u64)> {
        use devenv_activity::Activity;

        // Delete store paths using FFI
        // Strategy: Try to delete each path individually, skipping paths that
        // are still alive (referenced by other GC roots).
        //
        // We don't compute the full closure because shared dependencies would
        // fail to delete with "still alive" errors. Instead, we just try to
        // delete the top-level paths - Nix will only delete them if they're
        // truly unreferenced.

        if paths.is_empty() {
            return Ok((0, 0));
        }

        let mut store = (*self.store).clone();
        let mut total_deleted = 0u64;
        let mut total_bytes_freed = 0u64;
        let total_paths = paths.len() as u64;

        let activity = Activity::operation("Deleting store paths").start();

        for (i, path) in paths.iter().enumerate() {
            let path_str = match path.to_str() {
                Some(s) => s,
                None => continue,
            };

            // Extract just the name from store path for display
            let path_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(path_str);

            activity.progress(i as u64, total_paths, Some(path_name));

            let store_path = match store.parse_store_path(path_str).to_miette() {
                Ok(sp) => sp,
                Err(_) => {
                    // Not a valid store path, try to remove as regular file/directory
                    let _ = std::fs::remove_file(path).or_else(|_| std::fs::remove_dir_all(path));
                    continue;
                }
            };

            // Try to delete this specific path - if it's still alive, this will
            // fail gracefully and we skip it
            match store.collect_garbage(GcAction::DeleteSpecific, Some(&[&store_path]), false, 0) {
                Ok((deleted, bytes_freed)) => {
                    total_deleted += deleted.len() as u64;
                    total_bytes_freed += bytes_freed;
                }
                Err(_) => {
                    // Path is still alive (referenced by other roots), skip it
                    continue;
                }
            }
        }

        activity.progress(total_paths, total_paths, None);

        Ok((total_deleted, total_bytes_freed))
    }

    fn name(&self) -> &'static str {
        "nix"
    }

    async fn get_bash(&self, refresh_cached_output: bool) -> Result<String> {
        // Get the bash shell executable path for this system
        //
        // Evaluates and builds the bash attribute from default.nix,
        // which comes from locked nixpkgs and respects the system architecture.
        // Caches the result with a GC root at .devenv/bash-bash to avoid repeated builds.

        // The build() function appends the attribute name to the gc_root base path,
        // so we use "bash" as the base and the actual symlink becomes "bash-bash"
        let gc_root_base = self.paths.dotfile.join("bash");
        let gc_root_actual = self.paths.dotfile.join("bash-bash");

        // Try cache first
        if !refresh_cached_output
            && gc_root_actual.exists()
            && let Ok(cached_path) = std::fs::read_link(&gc_root_actual)
        {
            // Verify the path still exists in the store
            if cached_path.exists() {
                let path_str = cached_path.to_string_lossy().to_string();
                return Ok(format!("{path_str}/bin/bash"));
            }
        }

        // Cache miss or refresh requested - use build() which handles everything
        let paths = self
            .build(&["bash"], None, Some(&gc_root_base))
            .await
            .wrap_err("Failed to build bash attribute from default.nix")?;

        if paths.is_empty() {
            return Err(miette!("No output paths from bash build"));
        }

        // Return the path to the bash executable
        Ok(format!(
            "{store_path}/bin/bash",
            store_path = paths[0].to_string_lossy()
        ))
    }

    async fn is_trusted_user(&self) -> Result<bool> {
        // Check if the current user is trusted by the Nix daemon/store
        // This is used to determine if we can safely add substituters
        let mut store = (*self.store).clone();
        let trust_status = store.is_trusted_client();

        match trust_status {
            TrustedFlag::Trusted => Ok(true),
            TrustedFlag::NotTrusted => Ok(false),
            TrustedFlag::Unknown => Err(miette!(
                "Unable to determine trust status for Nix store (store type may not support trust queries)"
            )),
        }
    }

    fn invalidate(&self) {
        // Set the invalidation flag so the next dev_env() call bypasses cache
        self.cache_invalidated.store(true, Ordering::Release);

        // Replace the EvalState to clear C++ fileEvalCache.
        // The old EvalState caches evalFile() results by path, so reusing it
        // returns stale ASTs even when files have changed on disk.
        match self.create_fresh_eval_state() {
            Ok(new_state) => match self.eval_state.lock() {
                Ok(mut guard) => {
                    *guard = new_state;
                    tracing::debug!("EvalState replaced for hot-reload");
                }
                Err(e) => {
                    tracing::error!("Failed to lock eval state for replacement: {}", e);
                }
            },
            Err(e) => {
                tracing::error!("Failed to create fresh eval state for reload: {}", e);
            }
        }
    }
}

// Helper methods for NixRustBackend
impl NixRustBackend {
    /// Create a fresh EvalState, clearing all internal caches (e.g. fileEvalCache).
    /// Used during hot-reload to ensure changed files are re-evaluated.
    fn create_fresh_eval_state(&self) -> Result<EvalState> {
        let store = (*self.store).clone();
        let root_str = self
            .paths
            .root
            .to_str()
            .ok_or_else(|| miette!("Root path contains invalid UTF-8"))?;
        let nixpkgs_config_str = self
            .nixpkgs_config_path
            .to_str()
            .ok_or_else(|| miette!("Nixpkgs config path contains invalid UTF-8"))?;

        let builder = EvalStateBuilder::new(store)
            .to_miette()
            .wrap_err("Failed to create eval state builder")?
            .base_directory(root_str)
            .to_miette()
            .wrap_err("Failed to set base directory")?
            .env_override("NIXPKGS_CONFIG", nixpkgs_config_str)
            .to_miette()
            .wrap_err("Failed to set NIXPKGS_CONFIG")?
            .flakes(&self.flake_settings)
            .to_miette()
            .wrap_err("Failed to configure flakes")?;

        let mut eval_state = builder
            .build()
            .to_miette()
            .wrap_err("Failed to build eval state")?;

        if self.global_options.nix_debugger {
            eval_state
                .enable_debugger()
                .to_miette()
                .wrap_err("Failed to enable debugger")?;
        }

        Ok(eval_state)
    }
    /// Format lock file inputs as a tree structure
    fn format_lock_inputs(lock_file: &nix_bindings_flake::LockFile) -> Result<String> {
        let mut iter = lock_file
            .inputs_iterator()
            .to_miette()
            .wrap_err("Failed to create inputs iterator: {}")?;

        let mut inputs = Vec::new();

        // Collect all inputs
        while iter.next() {
            let attr_path = iter
                .attr_path()
                .to_miette()
                .wrap_err("Failed to get attr path")?;
            let locked_ref = iter
                .locked_ref()
                .to_miette()
                .wrap_err("Failed to get locked ref")?;

            // Only include top-level inputs (no "/" in path)
            if !attr_path.contains('/') {
                inputs.push((attr_path, locked_ref));
            }
        }

        if inputs.is_empty() {
            return Ok("Inputs:\n  (no inputs)".to_string());
        }

        // Sort inputs by name
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

            // Format brief reference info
            let brief_ref = Self::format_brief_ref(&ref_str);
            lines.push(format!("{prefix}{path}: {brief_ref}"));
        }

        Ok(lines.join("\n"))
    }

    /// Format a flake reference string into a brief display format
    fn format_brief_ref(ref_str: &str) -> String {
        // Parse reference like "github:NixOS/nixpkgs/6a08e6bb..." or "path:/nix/store/..."
        // Return in format "github:NixOS/nixpkgs/6a08e6b"

        if ref_str.is_empty() {
            return String::from("(follows)");
        }

        // Try to shorten the revision if it's a long hash
        if let Some(last_slash_idx) = ref_str.rfind('/') {
            let before_slash = &ref_str[..last_slash_idx];
            let after_slash = &ref_str[last_slash_idx + 1..];

            // If after slash looks like a hash (40+ chars, hex), truncate it
            if after_slash.len() >= 40 && after_slash.chars().all(|c| c.is_ascii_hexdigit()) {
                return format!("{}/{}", before_slash, &after_slash[..7]);
            }
        }

        ref_str.to_string()
    }

    /// Convert an FFI result to miette and enrich with any captured Nix error messages.
    ///
    /// This combines `.to_miette()` and error enrichment in one call, reducing boilerplate
    /// at call sites. When Nix evaluation fails, the detailed error (e.g., "failed to fetch
    /// input 'nixpkgs'") is often logged separately from the exception. This method combines
    /// the exception with any captured error logs to provide more helpful error messages.
    ///
    /// Uses `peek_pre_repl_errors()` so errors remain available for the debugger if needed.
    fn enriched<T>(&self, result: anyhow::Result<T>, context: impl AsRef<str>) -> Result<T> {
        result
            .to_miette()
            .map_err(|e| self.enrich_eval_error(e, context.as_ref()))
    }

    /// Enrich an existing miette error with any Nix error messages captured during evaluation.
    fn enrich_eval_error(&self, err: miette::Error, context: &str) -> miette::Error {
        let nix_errors = self.nix_log_bridge.peek_pre_repl_errors();

        if nix_errors.is_empty() {
            return err.wrap_err(context.to_string());
        }

        // Include the full last error which contains the complete Nix error with stack trace.
        // Wrap the original error to preserve the error chain.
        let nix_details = nix_errors.last().unwrap();
        err.wrap_err(format!("{}: {}", context, nix_details))
    }

    /// Evaluate a single attribute without caching.
    ///
    /// This is the pure evaluation logic extracted for use with transparent caching.
    fn eval_attr_uncached(
        &self,
        attr_path: &str,
        clean_path: &str,
        activity: &Activity,
    ) -> Result<String> {
        let mut eval_state = self.eval_session(activity)?;

        // Import default.nix with primops to get the attribute set
        let root_attrs = self.eval_import_with_primops(&mut eval_state)?;

        // Navigate to the attribute using the Nix API
        let value = self.enriched(
            eval_state.require_attrs_select(&root_attrs, clean_path),
            format!(
                "Failed to get attribute '{}' from devenv configuration",
                attr_path
            ),
        )?;

        // Force evaluation
        self.enriched(
            eval_state.force(&value),
            format!("Failed to force evaluation of '{}'", attr_path),
        )?;

        // Convert to JSON string
        let json_value = match value_to_json(&mut eval_state, &value) {
            Ok(v) => v,
            Err(e) => {
                // Log the full error to help debug port allocation errors
                tracing::error!(error = %e, "Failed to convert {} to JSON", attr_path);
                return Err(miette::miette!(
                    "Failed to convert {} to JSON: {}",
                    attr_path,
                    e
                ));
            }
        };

        serde_json::to_string(&json_value)
            .into_diagnostic()
            .wrap_err(format!("Failed to serialize {} to JSON", attr_path))
    }

    /// Build shell derivation without caching.
    ///
    /// This is the pure build logic extracted for use with transparent caching.
    fn build_shell_uncached(&self, activity: &Activity) -> Result<CachedShellPaths> {
        let mut eval_state = self.eval_session(activity)?;

        let devenv = self.eval_import_with_primops(&mut eval_state)?;

        // Get the shell derivation from devenv.shell
        let shell_drv = self.enriched(
            eval_state.require_attrs_select(&devenv, "shell"),
            "Failed to get shell attribute from devenv",
        )?;

        // Force evaluation to ensure the derivation is fully evaluated
        self.enriched(
            eval_state.force(&shell_drv),
            "Failed to force evaluation of shell derivation",
        )?;

        // Get drvPath
        let drv_path_value = self.enriched(
            eval_state.require_attrs_select(&shell_drv, "drvPath"),
            "Failed to get drvPath from shell derivation",
        )?;

        let drv_path = self.enriched(
            eval_state.require_string(&drv_path_value),
            "Failed to extract drvPath as string",
        )?;

        // Get outPath for building - it has string context
        let out_path_value = self.enriched(
            eval_state.require_attrs_select(&shell_drv, "outPath"),
            "Failed to get outPath from shell derivation",
        )?;

        // Build the derivation to get the output path
        let realized = self.enriched(
            eval_state.realise_string(&out_path_value, false),
            "Failed to realize shell derivation",
        )?;

        let store_path = realized
            .paths
            .first()
            .ok_or_else(|| miette!("Shell derivation produced no output paths"))?;

        let mut store = (*self.store).clone();
        let out_path = store
            .real_path(store_path)
            .to_miette()
            .wrap_err("Failed to get store path")?;

        // Get the -env output path from FFI.
        // This builds the -env derivation and returns both the env path and the environment.
        let drv_store_path = store
            .parse_store_path(&drv_path)
            .to_miette()
            .wrap_err("Failed to parse derivation store path")?;
        let (_build_env, env_store_path) =
            BuildEnvironment::get_dev_environment(&self.store, &drv_store_path)
                .to_miette()
                .wrap_err("Failed to get dev environment")?;

        // Convert the env store path to a real filesystem path for caching
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

    /// Build a single attribute without caching.
    ///
    /// This is the pure build logic extracted for use with transparent caching.
    fn build_attr_uncached(&self, attr_path: &str, activity: &Activity) -> Result<String> {
        let mut eval_state = self.eval_session(activity)?;

        let root_attrs = self.eval_import_with_primops(&mut eval_state)?;

        // Navigate to the attribute and force evaluation
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

        // If it's a derivation (attrs with .outPath), get the outPath
        // Otherwise use the value as-is (might be a string already)
        let build_value = self
            .enriched(
                eval_state.require_attrs_select_opt(&value, "outPath"),
                format!("Failed to check for outPath in attribute: {}", attr_path),
            )?
            .unwrap_or_else(|| value.clone());

        // Realize the value, which triggers the actual build
        let realized = self.enriched(
            eval_state.realise_string(&build_value, false),
            format!("Failed to build attribute: {}", attr_path),
        )?;

        let store_path = realized
            .paths
            .first()
            .ok_or_else(|| miette!("Attribute '{}' produced no output paths", attr_path))?;

        let mut store = (*self.store).clone();
        let path_str = store
            .real_path(store_path)
            .to_miette()
            .wrap_err("Failed to get store path")?;

        Ok(path_str)
    }

    /// Build the dev environment from scratch using FFI.
    ///
    /// This is the slow path that computes the environment when no cached
    /// env JSON is available. It builds a modified derivation that runs
    /// setup hooks and captures the resulting environment.
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

        // Use the FFI function to get the fully-expanded dev environment
        // This builds a modified derivation that runs setup hooks and captures the result
        let (mut build_env, _env_path) =
            BuildEnvironment::get_dev_environment(&self.store, &drv_store_path)
                .to_miette()
                .wrap_err("Failed to get dev environment from derivation")?;

        if json {
            build_env
                .to_json()
                .to_miette()
                .wrap_err("Failed to serialize environment to JSON")
        } else {
            // Get JSON from FFI, parse with Rust, and generate activation script
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
}

/// Cached shell paths for dev_env caching.
#[derive(serde::Serialize, serde::Deserialize)]
struct CachedShellPaths {
    drv_path: String,
    out_path: String,
    /// Path to the -env derivation output containing the environment JSON.
    /// When present and valid, we can skip the expensive FFI call.
    #[serde(default)]
    env_path: Option<String>,
}

impl Drop for NixRustBackend {
    fn drop(&mut self) {
        // Trigger shutdown to signal the cleanup task to run and exit.
        // This is sync (doesn't wait), but ensures the task doesn't stay
        // orphaned waiting for a shutdown that never comes.
        // The cleanup task will run its cleanup code and then exit.
        // Callers who want to wait for cleanup should call
        // shutdown.wait_for_shutdown_complete().await before dropping.
        self.shutdown.shutdown();
    }
}
