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
pub use tasks::{Tasks, TasksBuilder, compute_display_hierarchy};
pub use types::{
    Outputs, TaskCompleted, TaskOutputs, TaskStatus, TasksStatus, UiMode, VerbosityLevel,
    determine_ui_mode, is_tty,
};
pub use ui::TasksUi;

#[cfg(test)]
mod tests;
