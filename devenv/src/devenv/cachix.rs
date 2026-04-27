//! Cachix integration: runtime side.
//!
//! Configuration (netrc, trusted keys, [`StoreSettings`]) lives on
//! [`devenv_core::CachixManager`]. This module owns the runtime pieces
//! that only exist when a project actually uses cachix:
//!
//! - the post-init evaluation of `config.cachix.{enable,pull,push}`
//!   that produces a [`CachixCacheInfo`]
//! - the application of substituters/keys to the open store
//! - the optional push daemon ([`OwnedDaemon`]) — the daemon owns its own
//!   per-batch [`Activity`] internally, lazily started when work appears
//!   and dropped when the queue drains
//! - the realized-paths observer that streams paths from the backend
//!   into the daemon via an unbounded mpsc + a draining "pump" task
//! - the shutdown finalizer that drains the daemon before exit
//!
//! Kept in the orchestration crate (`devenv`) because [`OwnedDaemon`]
//! lives in `devenv-nix-backend` and the integration is C-Nix-specific
//! today; it is not part of the [`Evaluator`] trait surface.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use devenv_activity::{Activity, ActivityInstrument, ActivityLevel, activity, start};
use devenv_core::BuildOptions;
use devenv_core::cachix::{CachixCacheInfo, CachixManager};
use devenv_core::evaluator::Evaluator;
use devenv_core::realized::RealizedPathsObserver;
use devenv_core::settings::NixSettings;
use devenv_nix_backend::NixCBackend;
use devenv_nix_backend::cachix_daemon::{ConnectionParams, DaemonSpawnConfig, OwnedDaemon};
use miette::{IntoDiagnostic, Result, WrapErr};
use tokio::sync::{Mutex, mpsc, oneshot};
use tracing::{debug, info, warn};

/// Runtime cachix integration.
///
/// Constructed by [`CachixIntegration::init`] after the backend is up.
/// `init` returns `None` when cachix is disabled (`offline` mode or
/// `config.cachix.enable = false`); in that case the backend has no
/// observer attached and the per-realization notify is a one-branch
/// no-op.
pub struct CachixIntegration {
    /// `Some` only when `config.cachix.push` is set and the daemon
    /// spawned successfully. `take`n by the shutdown task at the end
    /// of the program.
    #[allow(dead_code)]
    daemon: Arc<Mutex<Option<OwnedDaemon>>>,
    /// Pump task draining the observer mpsc into `daemon.queue_paths`.
    /// Lives for the lifetime of `Devenv`.
    #[allow(dead_code)]
    pump: tokio::task::JoinHandle<()>,
    /// Shutdown finalizer task. Lives until the shutdown token fires,
    /// then drains the daemon and signals
    /// `shutdown.wait_for_shutdown_complete()`.
    #[allow(dead_code)]
    finalizer: tokio::task::JoinHandle<()>,
}

impl CachixIntegration {
    /// Eval cachix config, apply substituters, optionally spawn the
    /// push daemon and register a realized-paths observer on the
    /// backend.
    ///
    /// Returns `Ok(None)` when cachix is offline or disabled — in that
    /// case nothing is wired up and the backend pays a single
    /// vec-empty branch per realization.
    pub async fn init(
        cnix: &NixCBackend,
        cachix_manager: &Arc<CachixManager>,
        nix_settings: &NixSettings,
        shutdown: &Arc<tokio_shutdown::Shutdown>,
    ) -> Result<Option<Self>> {
        if nix_settings.offline {
            debug!("cachix: offline mode, skipping");
            return Ok(None);
        }

        // Errors propagate — a broken `config.cachix.enable` means a broken
        // devenv.nix, not "cachix is off".
        let enable: bool = eval_field(cnix, "config.cachix.enable").await?;
        if !enable {
            return Ok(None);
        }

        let push: Option<String> = async {
            let pull: Vec<String> = eval_field(cnix, "config.cachix.pull").await?;
            let push: Option<String> = eval_field(cnix, "config.cachix.push").await?;

            let known_keys = load_known_keys(&cachix_manager.paths.trusted_keys).await;
            let info = CachixCacheInfo {
                caches: devenv_core::cachix::Cachix {
                    pull,
                    push: push.clone(),
                },
                known_keys,
            };

            // Substituters/keys: build StoreSettings and apply additively
            // to the open store. Errors here are warn-only — devenv
            // continues without the substituter rather than failing the
            // whole command.
            match cachix_manager.store_settings(Some(&info)).await {
                Ok(settings) => cnix.apply_store_settings(&settings),
                Err(e) => warn!("cachix: failed to build store settings: {e}"),
            }

            Ok::<Option<String>, miette::Report>(push)
        }
        .in_activity(&activity!(INFO, operation, "Configuring cachix"))
        .await?;

        let Some(push_cache) = push else {
            return Ok(None);
        };

        let binary = match resolve_cachix_binary(cnix).await {
            Ok(b) => b,
            Err(e) => {
                warn!("cachix: failed to resolve cachix binary, push disabled: {e}");
                return Ok(None);
            }
        };

        let socket_path = cachix_manager
            .paths
            .daemon_socket
            .clone()
            .unwrap_or_else(|| {
                std::env::temp_dir().join(format!("cachix-daemon-{}.sock", std::process::id()))
            });

        let spawn_config = DaemonSpawnConfig {
            cache_name: push_cache.clone(),
            socket_path,
            binary,
            dry_run: false,
        };

        let owned = match OwnedDaemon::spawn(spawn_config, ConnectionParams::default()).await {
            Ok(d) => d,
            Err(e) => {
                warn!("cachix: failed to spawn daemon, push disabled: {e}");
                return Ok(None);
            }
        };

        info!("cachix: push daemon spawned for cache '{push_cache}'");

        let daemon = Arc::new(Mutex::new(Some(owned)));

        // Pump: drain unbounded mpsc into daemon.queue_paths. The
        // observer is sync and fire-and-forget; the pump turns that
        // into the daemon's async API.
        let (tx, mut rx) = mpsc::unbounded_channel::<Vec<PathBuf>>();
        let pump = {
            let daemon = daemon.clone();
            tokio::spawn(async move {
                while let Some(paths) = rx.recv().await {
                    let strings: Vec<String> = paths
                        .into_iter()
                        .filter_map(|p| p.into_os_string().into_string().ok())
                        .collect();
                    if strings.is_empty() {
                        continue;
                    }
                    let guard = daemon.lock().await;
                    if let Some(d) = guard.as_ref()
                        && let Err(e) = d.queue_paths(strings).await
                    {
                        warn!("cachix: failed to queue paths: {e}");
                    }
                }
            })
        };

        cnix.add_realized_observer(Arc::new(MpscObserver { tx }));

        // Shutdown finalizer: wait for cancellation, drain daemon, signal cleanup.
        let (cleanup_tx, cleanup_rx) = oneshot::channel::<()>();
        shutdown.set_cleanup_receiver(cleanup_rx);
        let finalizer = {
            let daemon = daemon.clone();
            let token = shutdown.cancellation_token();
            tokio::spawn(async move {
                token.cancelled().await;
                let taken = daemon.lock().await.take();
                if let Some(d) = taken {
                    info!("cachix: finalizing pushes...");
                    if let Err(e) = d.shutdown(Duration::from_secs(300)).await {
                        warn!("cachix: error during daemon shutdown: {e}");
                    }
                }
                let _ = cleanup_tx.send(());
            })
        };

        Ok(Some(Self {
            daemon,
            pump,
            finalizer,
        }))
    }
}

struct MpscObserver {
    tx: mpsc::UnboundedSender<Vec<PathBuf>>,
}

impl RealizedPathsObserver for MpscObserver {
    fn on_realized(&self, paths: &[PathBuf]) {
        // `send` on UnboundedSender is sync, never blocks, never
        // awaits. Returns Err only if the receiver is dropped — which
        // means shutdown has happened or the pump panicked. Either
        // way: ignore. The build still succeeds; we just won't push.
        let _ = self.tx.send(paths.to_vec());
    }
}

/// Evaluate `attr` and deserialize. The "Reading {attr}" activity is at Debug
/// level — these are internal evaluation steps, not user-facing operations.
async fn eval_field<T: serde::de::DeserializeOwned>(cnix: &NixCBackend, attr: &str) -> Result<T> {
    let activity =
        start!(Activity::evaluate(format!("Reading {attr}")).level(ActivityLevel::Debug));
    let json = cnix.eval_attr(attr, &activity).await?;
    serde_json::from_str(&json)
        .into_diagnostic()
        .wrap_err_with(|| format!("Failed to deserialize {attr}"))
}

async fn load_known_keys(path: &Path) -> BTreeMap<String, String> {
    match tokio::fs::read_to_string(path).await {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => BTreeMap::new(),
    }
}

async fn resolve_cachix_binary(cnix: &NixCBackend) -> Result<PathBuf> {
    if let Ok(p) = which::which("cachix") {
        return Ok(p);
    }
    // Fallback: build the cachix package from the user's module and
    // read its binary path. This forces the cachix derivation, which
    // is why the eval-each-field-separately rule applies (we never
    // evaluate `config.cachix` as a whole).
    let binary_path: PathBuf = eval_field(cnix, "config.cachix.binary").await?;
    cnix.build(&["config.cachix.package"], BuildOptions::default())
        .await
        .wrap_err("Failed to build config.cachix.package")?;
    Ok(binary_path)
}
