mod builder;
mod config;
pub mod coordinator;
mod manager;
mod pty;
mod watcher;

pub use builder::{BuildContext, BuildError, BuildTrigger, ShellBuilder};
pub use config::Config;
pub use coordinator::{
    CoordinatorError, PtyTaskRequest, PtyTaskResult, ShellCommand, ShellCoordinator, ShellEvent,
};
pub use manager::{ManagerError, ManagerMessage, ShellManager};
pub use portable_pty::CommandBuilder;

// Expose internal types for integration tests
#[doc(hidden)]
pub use pty::{get_terminal_size, Pty, PtyError};
pub use watcher::WatcherHandle;
#[doc(hidden)]
pub use watcher::{FileChangeEvent, FileWatcher, WatcherError};
