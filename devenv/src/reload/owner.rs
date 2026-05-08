//! Actor that owns the `Devenv` instance during a hot-reload shell session.
//!
//! `Devenv` method futures hold raw Nix C bindings across awaits and are
//! therefore `!Send` — they can't run on the main multi-thread runtime. The
//! owner runs on a dedicated thread with its own current-thread runtime,
//! which also outlives the main runtime so `spawn_blocking` callers can
//! still reach it during shutdown.

use crate::Devenv;
use crate::devenv::{ShellCommand, format_shell_exports};
use devenv_core::VerbosityLevel;
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
/// `verbosity` controls how the enterShell tasks re-run during reload report
/// progress.
///
/// Returns the `Devenv` once all `DevenvClient`s drop and the request channel
/// closes.
pub fn spawn_owner(
    devenv: Devenv,
    verbosity: VerbosityLevel,
) -> (DevenvClient, JoinHandle<Devenv>) {
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
                    handle_request(&devenv, req, verbosity).await;
                }
                devenv
            })
        })
        .expect("failed to spawn Devenv owner thread");
    (DevenvClient { tx }, handle)
}

/// Drop paths that should never end up in the reload watch set.
///
/// Excludes:
/// - missing files (likely deleted between eval and reload setup)
/// - `/nix/store` paths (immutable)
/// - anything inside the devenv dotfile dir *except* the `state/` subdir
///   (`$DEVENV_STATE`). Devenv churns most of the dotfile on every
///   evaluation (eval cache + WAL/SHM, tasks DB, generated shell scripts,
///   `imports.txt`, …); watching those would self-trigger reloads in an
///   infinite loop and propagate spurious changes when `lib.fileset`
///   `readDir`s a parent of `.devenv/`. The `state/` subdir is the
///   documented user-write area and stays watchable, minus a handful of
///   devenv-managed leaves under it (tasks DB + WAL/SHM, git-hooks state)
///   that would also self-trigger reloads — these mirror the eval cache's
///   longest-prefix-match exclusions in `CachingConfig` and act as
///   defense-in-depth in case a stale eval-cache row predates the filter.
fn is_watchable_input(path: &Path, dotfile: &Path) -> bool {
    if !path.exists() || path.starts_with("/nix/store") {
        return false;
    }
    if !path.starts_with(dotfile) {
        return true;
    }
    let state = dotfile.join("state");
    if !path.starts_with(&state) {
        return false;
    }
    // `Path::starts_with` matches at component boundaries, so each sqlite
    // sibling (`-wal`, `-shm`) needs its own entry.
    for internal in [
        state.join("tasks.db"),
        state.join("tasks.db-wal"),
        state.join("tasks.db-shm"),
        state.join("git-hooks"),
    ] {
        if path.starts_with(&internal) {
            return false;
        }
    }
    true
}

async fn handle_request(devenv: &Devenv, req: DevenvRequest, verbosity: VerbosityLevel) {
    match req {
        DevenvRequest::PrepareExec { cmd, args, reply } => {
            let _ = reply.send(devenv.prepare_exec(cmd, &args).await);
        }
        DevenvRequest::BuildReloadEnv {
            reload_file,
            watcher,
            reply,
        } => {
            let _ =
                reply.send(run_build_reload_env(devenv, &reload_file, &watcher, verbosity).await);
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
    verbosity: VerbosityLevel,
) -> Result<(), BuildError> {
    devenv
        .invalidate_for_reload()
        .await
        .map_err(|e| BuildError::new(format!("Failed to invalidate state for reload: {}", e)))?;

    let mut env_script = devenv
        .print_dev_env(false)
        .await
        .map_err(|e| BuildError::new(format!("Failed to build environment: {}", e)))?;

    // Re-run enterShell tasks so task-managed state (e.g. files created by the
    // `files` option via devenv:files) is refreshed on reload, matching what
    // happens on a fresh shell entry. Append their env exports so changes like
    // an updated PATH propagate into the live shell.
    let (exports, _messages) = devenv
        .run_enter_shell_tasks(None, verbosity)
        .await
        .map_err(|e| BuildError::new(format!("Failed to run enterShell tasks: {}", e)))?;
    env_script.push_str(&format_shell_exports(&exports));

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
    let dotfile = devenv.dotfile().to_path_buf();

    if let Some(cache_key) = devenv.shell_cache_key()
        && watch_by_key(pool, &cache_key.key_hash, watcher, &dotfile).await
    {
        return;
    }

    watch_all_tracked(pool, watcher, &dotfile).await;
}

async fn watch_by_key(
    pool: &sqlx::SqlitePool,
    key_hash: &str,
    watcher: &WatcherHandle,
    dotfile: &Path,
) -> bool {
    match devenv_eval_cache::get_file_inputs_by_key_hash(pool, key_hash).await {
        Ok(inputs) if !inputs.is_empty() => {
            let paths: Vec<PathBuf> = inputs
                .into_iter()
                .map(|i| i.path)
                .filter(|p| is_watchable_input(p, dotfile))
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

async fn watch_all_tracked(pool: &sqlx::SqlitePool, watcher: &WatcherHandle, dotfile: &Path) {
    match devenv_eval_cache::get_all_tracked_file_paths(pool).await {
        Ok(paths) => {
            let filtered: Vec<PathBuf> = paths
                .into_iter()
                .filter(|p| is_watchable_input(p, dotfile))
                .collect();
            watcher.watch_many(filtered).await;
        }
        Err(e) => {
            tracing::warn!("Failed to query all tracked files: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn rejects_devenv_internal_files() {
        // Everything devenv writes under the dotfile (eval-cache SQLite +
        // WAL/SHM, tasks DB, generated shell scripts, imports.txt, etc.)
        // would otherwise drag devenv churn into the reload watcher and
        // into Nix tracked inputs whenever `lib.fileset` `readDir`s a
        // parent of `.devenv/`.
        let temp = TempDir::new().unwrap();
        let dotfile = temp.path().join(".devenv");
        std::fs::create_dir_all(&dotfile).unwrap();

        for internal in [
            "nix-eval-cache.db",
            "nix-eval-cache.db-wal",
            "nix-eval-cache.db-shm",
            "tasks.db",
            "tasks.db-wal",
            "imports.txt",
            "shell-env.sh",
            "input-paths.txt",
        ] {
            let path = dotfile.join(internal);
            std::fs::write(&path, b"").unwrap();
            assert!(
                !is_watchable_input(&path, &dotfile),
                "{} must be excluded from watch set",
                internal
            );
        }
    }

    #[test]
    fn rejects_devenv_internal_subdirs() {
        // Subdirs devenv generates (bootstrap/, gc/) must also be filtered.
        let temp = TempDir::new().unwrap();
        let dotfile = temp.path().join(".devenv");
        let bootstrap = dotfile.join("bootstrap");
        std::fs::create_dir_all(&bootstrap).unwrap();
        let lib = bootstrap.join("bootstrapLib.nix");
        std::fs::write(&lib, b"{}").unwrap();

        assert!(!is_watchable_input(&lib, &dotfile));
    }

    #[test]
    fn rejects_nix_store_and_missing_paths() {
        let temp = TempDir::new().unwrap();
        let dotfile = temp.path().join(".devenv");
        std::fs::create_dir_all(&dotfile).unwrap();

        assert!(!is_watchable_input(
            Path::new("/nix/store/abc-foo/x.nix"),
            &dotfile
        ));
        assert!(!is_watchable_input(
            &temp.path().join("does-not-exist"),
            &dotfile
        ));
    }

    #[test]
    fn accepts_real_user_input_files() {
        let temp = TempDir::new().unwrap();
        let dotfile = temp.path().join(".devenv");
        std::fs::create_dir_all(&dotfile).unwrap();

        let user_file = temp.path().join("devenv.nix");
        std::fs::write(&user_file, b"{}").unwrap();

        assert!(is_watchable_input(&user_file, &dotfile));
    }

    #[test]
    fn accepts_files_under_devenv_state() {
        // `$DEVENV_STATE` lives at `<dotfile>/state/`. User-owned files
        // there (service data, mkcert root, etc.) stay watchable — that's
        // the carve-out from the broader dotfile exclusion.
        let temp = TempDir::new().unwrap();
        let dotfile = temp.path().join(".devenv");
        let state_dir = dotfile.join("state");
        std::fs::create_dir_all(state_dir.join("mkcert")).unwrap();

        let state_file = state_dir.join("mkcert/rootCA.pem");
        std::fs::write(&state_file, b"").unwrap();

        assert!(is_watchable_input(&state_file, &dotfile));
    }

    #[test]
    fn rejects_devenv_managed_state_leaves() {
        // The `state/` carve-out re-admits everything under it by default.
        // devenv-managed leaves churn on every task run / reload and would
        // self-trigger reloads if left in the watch set — they must be
        // dropped despite living under the exception.
        let temp = TempDir::new().unwrap();
        let dotfile = temp.path().join(".devenv");
        let state_dir = dotfile.join("state");
        std::fs::create_dir_all(state_dir.join("git-hooks")).unwrap();

        for internal in [
            "tasks.db",
            "tasks.db-wal",
            "tasks.db-shm",
            "git-hooks/config.json",
        ] {
            let path = state_dir.join(internal);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&path, b"").unwrap();
            assert!(
                !is_watchable_input(&path, &dotfile),
                "{} must be excluded from watch set",
                internal
            );
        }
    }
}
