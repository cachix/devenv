mod builder;
mod config;
pub mod coordinator;
mod manager;
mod watcher;

pub use builder::{BuildContext, BuildError, BuildTrigger, ShellBuilder};
pub use config::Config;
pub use coordinator::{CoordinatorError, ShellCoordinator};
pub use manager::{ManagerError, ManagerMessage, ShellManager};
pub use watcher::WatcherHandle;

// Re-export types from devenv-shell for backwards compatibility
pub use devenv_shell::{
    CommandBuilder, Pty, PtyError, PtyTaskRequest, PtyTaskResult, ShellCommand, ShellEvent,
    get_terminal_size,
};

// Expose internal types for integration tests
#[doc(hidden)]
pub use watcher::{FileChangeEvent, FileWatcher, WatcherError};
