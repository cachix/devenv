use crate::watcher::WatcherHandle;
use portable_pty::CommandBuilder;
use std::collections::HashMap;
use std::path::PathBuf;
use thiserror::Error;

/// What triggered a build
#[derive(Debug, Clone)]
pub enum BuildTrigger {
    /// Initial shell spawn
    Initial,
    /// File changed
    FileChanged(PathBuf),
}

/// Context passed to builder on each build
#[derive(Clone)]
pub struct BuildContext {
    /// Current working directory
    pub cwd: PathBuf,
    /// Current environment variables
    pub env: HashMap<String, String>,
    /// What triggered this build
    pub trigger: BuildTrigger,
    /// Handle to add new watch paths at runtime
    pub watcher: WatcherHandle,
}

/// Error returned by shell builder
#[derive(Debug, Error)]
#[error("{message}")]
pub struct BuildError {
    pub message: String,
    pub details: Option<String>,
}

impl BuildError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            details: None,
        }
    }
}

/// Trait for shell builders - implemented by the consumer (e.g., devenv)
pub trait ShellBuilder: Send + Sync {
    fn build(&self, ctx: &BuildContext) -> Result<CommandBuilder, BuildError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_error_new() {
        let err = BuildError::new("test error");
        assert_eq!(err.message, "test error");
        assert!(err.details.is_none());
    }

    #[test]
    fn test_build_error_display() {
        let err = BuildError::new("something failed");
        assert_eq!(format!("{}", err), "something failed");
    }

    #[test]
    fn test_build_trigger_initial_debug() {
        let trigger = BuildTrigger::Initial;
        let debug = format!("{:?}", trigger);
        assert!(debug.contains("Initial"));
    }

    #[test]
    fn test_build_trigger_file_changed_debug() {
        let trigger = BuildTrigger::FileChanged(PathBuf::from("/test/path.nix"));
        let debug = format!("{:?}", trigger);
        assert!(debug.contains("FileChanged"));
        assert!(debug.contains("path.nix"));
    }

    #[test]
    fn test_build_trigger_clone() {
        let trigger = BuildTrigger::FileChanged(PathBuf::from("test.nix"));
        let cloned = trigger.clone();
        match cloned {
            BuildTrigger::FileChanged(p) => assert_eq!(p, PathBuf::from("test.nix")),
            _ => panic!("wrong variant"),
        }
    }
}
