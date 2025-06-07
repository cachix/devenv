use miette::Diagnostic;
use std::fmt::Display;
use thiserror::Error;

#[derive(Error, Diagnostic, Debug)]
pub enum Error {
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error(transparent)]
    CacheError(#[from] devenv_cache_core::error::CacheError),
    TaskNotFound(String),
    MissingCommand(String),
    TasksNotFound(Vec<(String, String)>),
    InvalidTaskName(String),
    // TODO: be more precies where the cycle happens
    CycleDetected(String),
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::IoError(e) => write!(f, "IO Error: {}", e),
            Error::CacheError(e) => write!(f, "Cache Error: {}", e),
            Error::TasksNotFound(tasks) => write!(
                f,
                "Task dependencies not found: {}",
                tasks
                    .iter()
                    .map(|(task, dep)| format!("{} is depending on non-existent {}", task, dep))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Error::TaskNotFound(task) => write!(f, "Task does not exist: {}", task),
            Error::CycleDetected(task) => write!(f, "Cycle detected at task: {}", task),
            Error::MissingCommand(task) => write!(
                f,
                "Task {} defined a status, but is missing a command",
                task
            ),
            Error::InvalidTaskName(task) => write!(
                f,
                "Invalid task name: {}, expected [a-zA-Z-_]+:[a-zA-Z-_]+",
                task
            ),
        }
    }
}
