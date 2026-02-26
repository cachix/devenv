//! Activity system initialization and global event channel handle.
//!
//! This module provides the core initialization mechanism for the activity tracking system.
//! It creates an unbounded channel for activity events and provides a handle for installing
//! the sender globally.
//!
//! # Usage
//!
//! ```rust,ignore
//! use devenv_activity::{init, Activity};
//!
//! // Initialize the activity system
//! let (rx, handle) = devenv_activity::init();
//!
//! // Install the global sender - activities will now be sent to this channel
//! handle.install();
//!
//! // Pass the receiver to your TUI or event processor
//! while let Some(event) = rx.recv().await {
//!     // Handle event
//! }
//! ```

use tokio::sync::mpsc;

use crate::events::ActivityEvent;
use crate::stack::ACTIVITY_SENDER;

/// Handle for registering the activity event channel.
///
/// This handle holds the sender side of the activity event channel.
/// Call [`install()`](Self::install) to register the sender globally,
/// enabling all [`Activity`](crate::Activity) instances to send events.
pub struct ActivityHandle {
    tx: mpsc::UnboundedSender<ActivityEvent>,
}

/// Guard that clears the global activity sender on drop.
/// Hold this for the lifetime of the activity channel's receiver.
pub struct ActivityGuard;

impl Drop for ActivityGuard {
    fn drop(&mut self) {
        if let Ok(mut sender) = ACTIVITY_SENDER.lock() {
            *sender = None;
        }
    }
}

impl ActivityHandle {
    /// Install this handle's sender as the global activity event channel.
    /// After calling this, all Activity events will be sent to this channel.
    /// Returns an [`ActivityGuard`] that clears the sender when dropped,
    /// allowing subsequent log/error calls to fall back to tracing.
    pub fn install(self) -> ActivityGuard {
        if let Ok(mut sender) = ACTIVITY_SENDER.lock() {
            *sender = Some(self.tx);
        }
        ActivityGuard
    }
}

/// Initialize the activity system.
/// Returns receiver for TUI and a handle for installing the channel.
///
/// Usage:
/// ```rust,ignore
/// let (rx, handle) = devenv_activity::init();
/// handle.install();  // Activities now send to this channel
/// // Pass rx to TUI
/// ```
pub fn init() -> (mpsc::UnboundedReceiver<ActivityEvent>, ActivityHandle) {
    let (tx, rx) = mpsc::unbounded_channel();
    (rx, ActivityHandle { tx })
}
