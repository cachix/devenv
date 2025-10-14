mod config;
mod error;
mod privileges;
mod task_cache;
mod task_state;
mod tasks;
mod types;
pub mod ui;

pub use config::{Config, RunMode, TaskConfig};
pub use error::Error;
pub use privileges::SudoContext;
pub use tasks::{Tasks, TasksBuilder};
pub use types::{
    DependencyKind, Outputs, TaskCompleted, TaskOutputs, TaskStatus, TaskType, TasksStatus, UiMode,
    VerbosityLevel, determine_ui_mode, is_tty,
};
pub use ui::TasksUi;

// Re-export process types from devenv-processes
pub use devenv_processes::{ListenKind, ListenSpec, RestartPolicy, SocketActivationConfig};

#[cfg(test)]
mod tests;
