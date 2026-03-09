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
pub use executor::{ExecutionContext, ExecutionResult, OutputCallback, SubprocessExecutor};
pub use privileges::SudoContext;
pub use tasks::{Tasks, TasksBuilder, compute_display_hierarchy};
pub use types::{
    DependencyKind, Outputs, PROCESS_TASK_PREFIX, TaskCompleted, TaskOutputs, TaskStatus, TaskType,
    TasksStatus, UiMode, VerbosityLevel, determine_ui_mode, get_devenv_env, is_tty,
};
pub use ui::TasksUi;

// Re-export process types from devenv-processes
pub use devenv_processes::{ListenKind, ListenSpec, RestartPolicy, SocketActivationConfig};

#[cfg(test)]
mod tests;
