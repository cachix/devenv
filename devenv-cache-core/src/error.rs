use miette::Diagnostic;
use std::path::PathBuf;
use thiserror::Error;

/// Common error type for cache operations
#[derive(Error, Diagnostic, Debug)]
pub enum CacheError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Failed to initialize cache: {0}")]
    Initialization(String),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("File not found: {0}")]
    FileNotFound(PathBuf),

    #[error("Environment variable not set: {0}")]
    MissingEnvVar(String),

    #[error("Invalid path: {0}")]
    InvalidPath(PathBuf),

    #[error("Content hash calculation failed for {path}: {reason}")]
    HashFailure { path: PathBuf, reason: String },
}

impl CacheError {
    /// Create a new initialization error
    pub fn initialization<S: ToString>(message: S) -> Self {
        Self::Initialization(message.to_string())
    }

    /// Create a new missing environment variable error
    pub fn missing_env_var<S: ToString>(var_name: S) -> Self {
        Self::MissingEnvVar(var_name.to_string())
    }
}

/// A specialized result type for cache operations
pub type CacheResult<T> = std::result::Result<T, CacheError>;
