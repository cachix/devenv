//! Activity system initialization and handle.

use tokio::sync::mpsc;

use crate::events::ActivityEvent;
use crate::stack::ACTIVITY_SENDER;

/// Handle for registering the activity event channel
pub struct ActivityHandle {
    tx: mpsc::Sender<ActivityEvent>,
}

impl ActivityHandle {
    /// Install this handle's sender as the global activity event channel.
    /// After calling this, all Activity events will be sent to this channel.
    pub fn install(self) {
        let _ = ACTIVITY_SENDER.set(self.tx);
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
pub fn init() -> (mpsc::Receiver<ActivityEvent>, ActivityHandle) {
    let (tx, rx) = mpsc::channel(32);
    (rx, ActivityHandle { tx })
}
