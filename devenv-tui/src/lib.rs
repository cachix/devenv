pub mod events;
pub mod model_events;
pub mod tracing_interface;
pub mod tracing_layer;

// UI modules
pub mod app;
pub mod components;
pub mod model;
pub mod view;

pub use events::*;
pub use model::{
    Model, Activity, ActivityVariant, BuildActivity, DownloadActivity, ProgressActivity,
    QueryActivity, TaskActivity, TaskDisplayStatus,
};
pub use model_events::{DataEvent, UiEvent};
pub use tracing_layer::DevenvTuiLayer;

use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// Handle for TUI system with event-based architecture
#[derive(Clone)]
pub struct TuiHandle {
    /// Model shared with rendering (read-only in practice)
    pub model: Arc<Mutex<Model>>,
    /// Data event sender (tracing events - high volume, batchable)
    data_tx: mpsc::UnboundedSender<DataEvent>,
    /// UI event sender (keyboard, tick - low volume, high priority)
    ui_tx: mpsc::UnboundedSender<UiEvent>,
}

impl TuiHandle {
    pub fn init() -> Self {
        let model = Arc::new(Mutex::new(Model::new()));
        let (data_tx, data_rx) = mpsc::unbounded_channel();
        let (ui_tx, ui_rx) = mpsc::unbounded_channel();

        // Spawn event processor task with two queues
        let model_clone = Arc::clone(&model);
        tokio::spawn(async move {
            process_events(data_rx, ui_rx, model_clone).await;
        });

        Self {
            model,
            data_tx,
            ui_tx,
        }
    }

    /// Get a clone of the model handle for rendering
    pub fn model(&self) -> Arc<Mutex<Model>> {
        self.model.clone()
    }

    /// Create a tracing layer that sends data events to the processor
    pub fn layer(&self) -> DevenvTuiLayer {
        DevenvTuiLayer::new(self.data_tx.clone())
    }

    /// Send a data event (from tracing layer)
    pub fn send_data_event(&self, event: DataEvent) {
        let _ = self.data_tx.send(event);
    }

    /// Send a UI event (from keyboard, timers, etc.)
    pub fn send_ui_event(&self, event: UiEvent) {
        let _ = self.ui_tx.send(event);
    }
}

/// Event processor task with two queues and priority handling
///
/// UI events are always processed first for responsiveness.
/// Data events are batched (up to 32) for efficiency.
async fn process_events(
    mut data_rx: mpsc::UnboundedReceiver<DataEvent>,
    mut ui_rx: mpsc::UnboundedReceiver<UiEvent>,
    model: Arc<Mutex<Model>>,
) {
    let mut data_batch = Vec::with_capacity(32);

    loop {
        tokio::select! {
            // Priority 1: UI events (always processed immediately)
            Some(ui_event) = ui_rx.recv() => {
                if let Ok(mut model_guard) = model.lock() {
                    ui_event.apply(&mut model_guard);
                }
            }

            // Priority 2: Data events (batched for efficiency)
            Some(data_event) = data_rx.recv() => {
                // Collect first event
                data_batch.push(data_event);

                // Drain remaining events without blocking (batching)
                while let Ok(event) = data_rx.try_recv() {
                    data_batch.push(event);
                    if data_batch.len() >= 32 {
                        break;
                    }
                }

                // Process batch with single lock
                if let Ok(mut model_guard) = model.lock() {
                    for event in data_batch.drain(..) {
                        event.apply(&mut model_guard);
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
