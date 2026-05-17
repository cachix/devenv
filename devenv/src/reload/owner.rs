//! Actor that owns the `Devenv` instance during a hot-reload shell session.
//!
//! `Devenv` method futures hold raw Nix C bindings across awaits and are
//! therefore `!Send` — they can't run on the main multi-thread runtime. The
//! owner runs on a dedicated thread with its own current-thread runtime,
//! which also outlives the main runtime so `spawn_blocking` callers can
//! still reach it during shutdown.

use crate::Devenv;
use crate::devenv::ShellCommand;
use devenv_nix_backend::{NIX_STACK_SIZE, gc_register_current_thread};
use devenv_reload::{BuildError, WatcherHandle};
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use tokio::sync::{mpsc, oneshot};

const OWNER_GONE: &str = "Devenv owner task is gone";
const REPLY_DROPPED: &str = "Devenv owner dropped reply";

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
        self.request(|reply| DevenvRequest::PrepareExec { cmd, args, reply })?
            .map_err(|e| BuildError::new(format!("Failed to prepare shell: {}", e)))
    }

    pub fn build_reload_env_blocking(
        &self,
        reload_file: PathBuf,
        watcher: WatcherHandle,
    ) -> Result<(), BuildError> {
        self.request(|reply| DevenvRequest::BuildReloadEnv {
            reload_file,
            watcher,
            reply,
        })?
    }

    pub fn add_watch_paths_blocking(&self, watcher: WatcherHandle) {
        if self
            .request(|reply| DevenvRequest::AddWatchPaths { watcher, reply })
            .is_err()
        {
            tracing::debug!("Skipping watch path refresh: owner unreachable");
        }
    }

    fn request<T>(
        &self,
        build: impl FnOnce(oneshot::Sender<T>) -> DevenvRequest,
    ) -> Result<T, BuildError> {
        let (reply, reply_rx) = oneshot::channel();
        self.tx
            .blocking_send(build(reply))
            .map_err(|_| BuildError::new(OWNER_GONE))?;
        reply_rx
            .blocking_recv()
            .map_err(|_| BuildError::new(REPLY_DROPPED))
    }
}

/// Spawn the owner on a dedicated thread.
///
/// Returns the `Devenv` once all `DevenvClient`s drop and the request channel
/// closes.
pub fn spawn_owner(devenv: Devenv) -> (DevenvClient, JoinHandle<Devenv>) {
    let (tx, mut rx) = mpsc::channel::<DevenvRequest>(32);
    let handle = std::thread::Builder::new()
        .name("devenv-reload-owner".into())
        .stack_size(NIX_STACK_SIZE)
        .spawn(move || {
            let _ = gc_register_current_thread();
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build current-thread runtime for Devenv owner");
            rt.block_on(async move {
                while let Some(req) = rx.recv().await {
                    handle_request(&devenv, req).await;
                }
                devenv
            })
        })
        .expect("failed to spawn Devenv owner thread");
    (DevenvClient { tx }, handle)
}

async fn handle_request(devenv: &Devenv, req: DevenvRequest) {
    match req {
        DevenvRequest::PrepareExec { cmd, args, reply } => {
            let _ = reply.send(devenv.prepare_exec(cmd, &args).await);
        }
        DevenvRequest::BuildReloadEnv {
            reload_file,
            watcher,
            reply,
        } => {
            let _ = reply.send(run_build_reload_env(devenv, &reload_file, &watcher).await);
        }
        DevenvRequest::AddWatchPaths { watcher, reply } => {
            add_watch_paths(devenv, &watcher).await;
            let _ = reply.send(());
        }
    }
}

async fn run_build_reload_env(
    devenv: &Devenv,
    reload_file: &Path,
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

/// Refresh the file watcher from the eval cache. Tries the current shell key
/// first, then falls back to all tracked files.
async fn add_watch_paths(devenv: &Devenv, watcher: &WatcherHandle) {
    let Some(pool) = devenv.eval_cache_pool() else {
        return;
    };

    if let Some(cache_key) = devenv.shell_cache_key()
        && watch_by_key(pool, &cache_key.key_hash, watcher).await
    {
        return;
    }

    watch_all_tracked(pool, watcher).await;
}

async fn watch_by_key(pool: &sqlx::SqlitePool, key_hash: &str, watcher: &WatcherHandle) -> bool {
    match devenv_eval_cache::get_file_inputs_by_key_hash(pool, key_hash).await {
        Ok(inputs) if !inputs.is_empty() => {
            let paths: Vec<PathBuf> = inputs
                .into_iter()
                .map(|i| i.path)
                .filter(|p| watchable(p))
                .collect();
            watcher.watch_many(paths).await;
            true
        }
        Ok(_) => false,
        Err(e) => {
            tracing::warn!("Failed to query eval cache by key_hash: {}", e);
            false
        }
    }
}

async fn watch_all_tracked(pool: &sqlx::SqlitePool, watcher: &WatcherHandle) {
    match devenv_eval_cache::get_all_tracked_file_paths(pool).await {
        Ok(paths) => {
            let filtered: Vec<PathBuf> = paths.into_iter().filter(|p| watchable(p)).collect();
            watcher.watch_many(filtered).await;
        }
        Err(e) => {
            tracing::warn!("Failed to query all tracked files: {}", e);
        }
    }
}

fn watchable(path: &Path) -> bool {
    path.exists() && !path.starts_with("/nix/store")
}
