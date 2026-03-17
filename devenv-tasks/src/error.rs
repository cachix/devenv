use miette::Diagnostic;
use thiserror::Error;

#[derive(Error, Diagnostic, Debug)]
pub enum Error {
    #[error("IO Error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Cache Error: {0}")]
    CacheError(#[from] devenv_cache_core::error::CacheError),
    #[error("Task does not exist: {0}")]
    TaskNotFound(String),
    #[error("Task {0} defined a status, but is missing a command")]
    MissingCommand(String),
    #[error("Task dependencies not found: {}", format_tasks_not_found(.0))]
    TasksNotFound(Vec<(String, String)>),
    #[error(
        "Invalid task name: {0}. Task names must be in format 'namespace:name' and can only contain alphanumeric characters, ':', '-', and '_'. The '@' character is reserved for dependency suffix notation."
    )]
    InvalidTaskName(String),
    #[error("{0}")]
    InvalidDependency(String),
    // TODO: be more precies where the cycle happens
    #[error("Cycle detected at task: {0}")]
    CycleDetected(String),
}

impl Error {
    pub fn io(msg: impl std::fmt::Display) -> Self {
        Self::IoError(std::io::Error::other(msg.to_string()))
    }
}

fn format_tasks_not_found(tasks: &[(String, String)]) -> String {
    tasks
        .iter()
        .map(|(task, dep)| format!("{task} is depending on non-existent {dep}"))
        .collect::<Vec<_>>()
        .join(", ")
}
