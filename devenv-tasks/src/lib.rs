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
pub use executor::{
    ExecutionContext, ExecutionResult, OutputCallback, PtyExecutor, SubprocessExecutor,
    TaskExecutor, default_executor,
};
pub use privileges::SudoContext;
pub use tasks::{Tasks, TasksBuilder};
pub use types::{
    Outputs, TaskCompleted, TaskOutputs, TaskStatus, TasksStatus, UiMode, VerbosityLevel,
    determine_ui_mode, is_tty,
};
pub use ui::TasksUi;

#[cfg(test)]
mod tests;
