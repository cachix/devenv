//! Actor that owns the `Devenv` instance during a hot-reload shell session.
//!
//! A dedicated thread owns the `Devenv` and serves requests over a channel;
//! sync `spawn_blocking` threads from the reload coordinator talk to it via
//! `blocking_send`.
//!
//! The owner runs on its own thread (with its own current-thread tokio
//! runtime) because `Devenv` method futures hold raw Nix C bindings across
//! awaits and are therefore `!Send` — they cannot be spawned onto the main
//! multi-thread runtime. The dedicated thread also outlives the main
//! runtime, so spawn_blocking workers can still talk to the owner during
//! shutdown without panicking on a torn-down runtime handle.

use crate::Devenv;
use crate::devenv::ShellCommand;
use devenv_reload::{BuildError, WatcherHandle};
use std::path::PathBuf;
use std::thread::JoinHandle;
use tokio::sync::{mpsc, oneshot};

pub enum DevenvRequest {
    PrepareExec {
        cmd: Option<String>,
        args: Vec<String>,
        reply: oneshot::Sender<miette::Result<ShellCommand>>,
    },
    BuildReloadEnv {
        reload_file: PathBuf,
        watcher: WatcherHandle,
        reply: oneshot::Sender<Result<(), BuildError>>,
    },
    AddWatchPaths {
        watcher: WatcherHandle,
        reply: oneshot::Sender<()>,
    },
}

pub struct DevenvClient {
    tx: mpsc::Sender<DevenvRequest>,
}

impl DevenvClient {
    pub fn prepare_exec_blocking(
        &self,
        cmd: Option<String>,
        args: Vec<String>,
    ) -> Result<ShellCommand, BuildError> {
        let (reply, reply_rx) = oneshot::channel();
        self.tx
            .blocking_send(DevenvRequest::PrepareExec { cmd, args, reply })
            .map_err(|_| BuildError::new("Devenv owner task is gone"))?;
        reply_rx
            .blocking_recv()
            .map_err(|_| BuildError::new("Devenv owner dropped reply"))?
            .map_err(|e| BuildError::new(format!("Failed to prepare shell: {}", e)))
    }

    pub fn build_reload_env_blocking(
        &self,
        reload_file: PathBuf,
        watcher: WatcherHandle,
    ) -> Result<(), BuildError> {
        let (reply, reply_rx) = oneshot::channel();
        self.tx
            .blocking_send(DevenvRequest::BuildReloadEnv {
                reload_file,
                watcher,
                reply,
            })
            .map_err(|_| BuildError::new("Devenv owner task is gone"))?;
        reply_rx
            .blocking_recv()
            .map_err(|_| BuildError::new("Devenv owner dropped reply"))?
    }

    pub fn add_watch_paths_blocking(&self, watcher: WatcherHandle) {
        let (reply, reply_rx) = oneshot::channel();
        if self
            .tx
            .blocking_send(DevenvRequest::AddWatchPaths { watcher, reply })
            .is_err()
        {
            tracing::debug!("Devenv owner gone, skipping watch path refresh");
            return;
        }
        if reply_rx.blocking_recv().is_err() {
            tracing::debug!("Devenv owner dropped reply, skipping watch path refresh");
        }
    }
}

/// Spawn the owner on a dedicated thread with its own current-thread tokio
/// runtime.
///
/// We can't host it on the main multi-thread runtime: `Devenv` method futures
/// hold raw Nix C bindings across awaits and are therefore `!Send`. A pinned
/// current-thread runtime avoids any Send requirement, and outlives the main
/// runtime so spawn_blocking threads can still talk to it during shutdown.
///
/// Returns the `Devenv` when the channel closes (i.e. when all `DevenvClient`
/// clones are dropped).
pub fn spawn_owner(devenv: Devenv) -> (DevenvClient, JoinHandle<Devenv>) {
    let (tx, mut rx) = mpsc::channel::<DevenvRequest>(32);
    let handle = std::thread::Builder::new()
        .name("devenv-reload-owner".into())
        .stack_size(devenv_nix_backend::NIX_STACK_SIZE)
        .spawn(move || {
            let _ = devenv_nix_backend::gc_register_current_thread();
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build current-thread runtime for Devenv owner");
            rt.block_on(async move {
                while let Some(req) = rx.recv().await {
                    match req {
                        DevenvRequest::PrepareExec { cmd, args, reply } => {
                            let result = devenv.prepare_exec(cmd, &args).await;
                            let _ = reply.send(result);
                        }
                        DevenvRequest::BuildReloadEnv {
                            reload_file,
                            watcher,
                            reply,
                        } => {
                            let result =
                                run_build_reload_env(&devenv, &reload_file, &watcher).await;
                            let _ = reply.send(result);
                        }
                        DevenvRequest::AddWatchPaths { watcher, reply } => {
                            add_watch_paths(&devenv, &watcher).await;
                            let _ = reply.send(());
                        }
                    }
                }
                devenv
            })
        })
        .expect("failed to spawn Devenv owner thread");
    (DevenvClient { tx }, handle)
}

async fn run_build_reload_env(
    devenv: &Devenv,
    reload_file: &std::path::Path,
    watcher: &WatcherHandle,
) -> Result<(), BuildError> {
    devenv
        .invalidate_for_reload()
        .await
        .map_err(|e| BuildError::new(format!("Failed to invalidate state for reload: {}", e)))?;

    let env_script = devenv
        .print_dev_env(false)
        .await
        .map_err(|e| BuildError::new(format!("Failed to build environment: {}", e)))?;

    let temp_path = reload_file.with_extension("sh.tmp");
    std::fs::write(&temp_path, &env_script)
        .map_err(|e| BuildError::new(format!("Failed to write pending env: {}", e)))?;
    std::fs::rename(&temp_path, reload_file)
        .map_err(|e| BuildError::new(format!("Failed to rename pending env: {}", e)))?;

    add_watch_paths(devenv, watcher).await;
    Ok(())
}

/// Refresh the file watcher from the eval cache.
async fn add_watch_paths(devenv: &Devenv, watcher: &WatcherHandle) {
    let Some(pool) = devenv.eval_cache_pool() else {
        tracing::trace!("No eval cache pool available");
        return;
    };

    if let Some(cache_key) = devenv.shell_cache_key() {
        tracing::trace!(
            "Looking up file inputs for key_hash: {}",
            cache_key.key_hash
        );
        match devenv_eval_cache::get_file_inputs_by_key_hash(pool, &cache_key.key_hash).await {
            Ok(inputs) if !inputs.is_empty() => {
                tracing::trace!("Found {} file inputs for shell key", inputs.len());
                let paths: Vec<_> = inputs
                    .into_iter()
                    .filter(|i| i.path.exists() && !i.path.starts_with("/nix/store"))
                    .map(|i| i.path)
                    .collect();
                watcher.watch_many(paths).await;
                return;
            }
            Ok(_) => {
                tracing::trace!("No file inputs found for shell key, trying all tracked files");
            }
            Err(e) => {
                tracing::warn!("Failed to query by key_hash: {}", e);
            }
        }
    }

    match devenv_eval_cache::get_all_tracked_file_paths(pool).await {
        Ok(paths) => {
            tracing::trace!("Found {} total tracked files in eval cache", paths.len());
            let filtered: Vec<_> = paths
                .into_iter()
                .filter(|p| p.exists() && !p.starts_with("/nix/store"))
                .collect();
            watcher.watch_many(filtered).await;
        }
        Err(e) => {
            tracing::warn!("Failed to query all tracked files: {}", e);
        }
    }
}
