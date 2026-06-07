mod config;
mod error;
pub mod executor;
mod privileges;
mod task_cache;
mod task_state;
mod tasks;
mod types;
pub mod ui;

pub use config::{Config, RunMode, TaskConfig};
pub use error::Error;
pub use executor::{ExecutionContext, ExecutionResult, OutputCallback};
pub use privileges::SudoContext;
pub use tasks::{Tasks, TasksBuilder, compute_display_hierarchy};
pub use types::{
    DependencyKind, Outputs, PROCESS_TASK_PREFIX, TaskCompleted, TaskOutputs, TaskStatus, TaskType,
    TasksStatus, UiMode, determine_ui_mode, get_devenv_env, is_tty,
};
pub use ui::TasksUi;

// Re-export process types from devenv-processes
pub use devenv_processes::{ListenKind, ListenSpec, RestartPolicy, SocketActivationConfig};

/// Pre-initialize the on-disk task cache for the given cache directory.
///
/// Creates the SQLite database, switches it to WAL mode, and applies
/// migrations. Running this once before spawning the per-process
/// `devenv-tasks` invocations (which a non-native process manager launches
/// concurrently) avoids those processes racing to create and migrate the same
/// database, which could surface as a connection pool timeout (#2897).
pub async fn warm_cache(cache_dir: &std::path::Path) -> Result<(), Error> {
    task_cache::TaskCache::new(cache_dir)
        .await
        .map(|_| ())
        .map_err(|e| Error::io(format!("Failed to initialize task cache: {e}")))
}

#[cfg(test)]
mod tests;
