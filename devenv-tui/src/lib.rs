pub mod events;
pub mod model_events;
pub mod tracing_interface;

// UI modules
pub mod app;
pub mod components;
pub mod model;
pub mod view;

pub use events::*;
pub use model::{
    Activity, ActivityVariant, BuildActivity, DownloadActivity, Model, ProgressActivity,
    QueryActivity, TaskActivity, TaskDisplayStatus,
};
pub use model_events::UiEvent;

use devenv_activity::ActivityEvent;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// Sender for UI events (keyboard, tick)
#[derive(Clone)]
pub struct UiSender(mpsc::UnboundedSender<UiEvent>);

impl UiSender {
    /// Send a UI event
    pub fn send(&self, event: UiEvent) {
        let _ = self.0.send(event);
    }
}

/// Event processor task with two queues and priority handling
///
/// UI events are always processed first for responsiveness.
/// Activity events are batched (up to 32) for efficiency.
pub(crate) async fn process_events(
    mut activity_rx: mpsc::UnboundedReceiver<ActivityEvent>,
    mut ui_rx: mpsc::UnboundedReceiver<UiEvent>,
    model: Arc<Mutex<Model>>,
) {
    let mut activity_batch = Vec::with_capacity(32);

    loop {
        tokio::select! {
            // Priority 1: UI events (always processed immediately)
            Some(ui_event) = ui_rx.recv() => {
                if let Ok(mut model_guard) = model.lock() {
                    ui_event.apply(&mut model_guard);
                }
            }

            // Priority 2: Activity events (batched for efficiency)
            Some(activity_event) = activity_rx.recv() => {
                // Collect first event
                activity_batch.push(activity_event);

                // Drain remaining events without blocking (batching)
                while let Ok(event) = activity_rx.try_recv() {
                    activity_batch.push(event);
                    if activity_batch.len() >= 32 {
                        break;
                    }
                }

                // Process batch with single lock
                if let Ok(mut model_guard) = model.lock() {
                    for event in activity_batch.drain(..) {
                        model_guard.apply_activity_event(event);
                    }
                }
            }

            // Both channels closed - exit
            else => break,
        }
    }
}

/// Create UI channel and return sender/receiver pair
pub(crate) fn create_ui_channel() -> (UiSender, mpsc::UnboundedReceiver<UiEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    (UiSender(tx), rx)
}
