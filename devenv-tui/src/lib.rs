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

/// Handle for TUI system with event-based architecture
#[derive(Clone)]
pub struct TuiHandle {
    /// Model shared with rendering (read-only in practice)
    pub model: Arc<Mutex<Model>>,
    /// Activity event sender (from devenv-activity)
    activity_tx: mpsc::UnboundedSender<ActivityEvent>,
    /// UI event sender (keyboard, tick - low volume, high priority)
    ui_tx: mpsc::UnboundedSender<UiEvent>,
}

impl TuiHandle {
    pub fn init() -> Self {
        let model = Arc::new(Mutex::new(Model::new()));
        let (activity_tx, activity_rx) = mpsc::unbounded_channel();
        let (ui_tx, ui_rx) = mpsc::unbounded_channel();

        // Spawn event processor task with two queues
        let model_clone = Arc::clone(&model);
        tokio::spawn(async move {
            process_events(activity_rx, ui_rx, model_clone).await;
        });

        Self {
            model,
            activity_tx,
            ui_tx,
        }
    }

    /// Get a clone of the model handle for rendering
    pub fn model(&self) -> Arc<Mutex<Model>> {
        self.model.clone()
    }

    /// Get the activity event sender for forwarding activity events
    pub fn activity_tx(&self) -> mpsc::UnboundedSender<ActivityEvent> {
        self.activity_tx.clone()
    }

    /// Send an activity event
    pub fn send_activity_event(&self, event: ActivityEvent) {
        let _ = self.activity_tx.send(event);
    }

    /// Send a UI event (from keyboard, timers, etc.)
    pub fn send_ui_event(&self, event: UiEvent) {
        let _ = self.ui_tx.send(event);
    }
}

/// Event processor task with two queues and priority handling
///
/// UI events are always processed first for responsiveness.
/// Activity events are batched (up to 32) for efficiency.
async fn process_events(
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

/// Initialize the TUI system and return handle
///
/// The TUI should be started manually using SubsystemBuilder::new()
pub fn init_tui() -> TuiHandle {
    TuiHandle::init()
}

/// Spawn a task that forwards ActivityEvents from the activity system to the TUI
pub fn spawn_activity_forwarder(
    mut activity_rx: mpsc::UnboundedReceiver<ActivityEvent>,
    activity_tx: mpsc::UnboundedSender<ActivityEvent>,
) {
    tokio::spawn(async move {
        while let Some(event) = activity_rx.recv().await {
            if activity_tx.send(event).is_err() {
                break;
            }
        }
    });
}
