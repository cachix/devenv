mod config;
mod error;
mod privileges;
pub mod signal_handler;
mod task_cache;
mod task_state;
mod tasks;
mod tracing_events;
mod types;
pub mod ui;

pub use config::{Config, RunMode, TaskConfig};
pub use error::Error;
pub use privileges::SudoContext;
pub use tasks::{Tasks, TasksBuilder};
pub use types::{
    determine_ui_mode, is_tty, Outputs, TaskCompleted, TaskOutputs, TaskStatus, TasksStatus,
    UiMode, VerbosityLevel,
};
pub use ui::{TasksUi, TasksUiBuilder};

#[cfg(test)]
mod tests;
