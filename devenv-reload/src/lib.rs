mod builder;
mod config;
pub mod coordinator;
mod manager;

pub use builder::{BuildContext, BuildError, BuildTrigger, ShellBuilder};
pub use config::Config;
pub use coordinator::{CoordinatorError, ShellCoordinator};
pub use manager::{ManagerError, ManagerMessage, ShellManager};

// Re-export file watcher types
pub use devenv_event_sources::{FileChangeEvent, FileWatcher, FileWatcherConfig, WatcherHandle};

// Re-export types from devenv-shell for backwards compatibility
pub use devenv_shell::{
    CommandBuilder, Pty, PtyError, PtyTaskRequest, PtyTaskResult, ShellCommand, ShellEvent,
    get_terminal_size,
};
