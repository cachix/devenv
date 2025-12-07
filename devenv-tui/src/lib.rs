pub mod model_events;
pub mod tracing_interface;

// UI modules
pub mod app;
pub mod components;
pub mod expanded_view;
pub mod model;
pub mod view;

pub use model::{
    Activity, ActivityVariant, BuildActivity, ChildActivityLimit, DownloadActivity, Model,
    ProgressActivity, QueryActivity, TaskActivity, TaskDisplayStatus, TerminalSize, ViewMode,
};
pub use model_events::UiEvent;

use devenv_activity::ActivityEvent;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// Event processor task with two queues and priority handling
///
/// UI events are always processed first for responsiveness.
/// Activity events are batched (up to 32) for efficiency.
pub(crate) async fn process_events(
    mut activity_rx: mpsc::Receiver<ActivityEvent>,
    mut ui_rx: mpsc::Receiver<UiEvent>,
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
