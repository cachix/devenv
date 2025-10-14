pub mod events;
pub mod tracing_layer;

// UI modules
pub mod app;
pub mod components;
pub mod model;
pub mod view;

pub use events::*;
pub use tracing_layer::DevenvTuiLayer;

use crate::model::Model;
use std::sync::{Arc, Mutex};

/// Handle for TUI system with proper shutdown tracking
pub struct TuiHandle {
    pub layer: DevenvTuiLayer,
    pub model: Arc<Mutex<Model>>,
}

impl TuiHandle {
    /// Get a clone of the model handle
    pub fn model(&self) -> Arc<Mutex<Model>> {
        self.model.clone()
    }
}

/// Initialize the TUI system and return handle
///
/// The TUI should be started manually using SubsystemBuilder::new()
pub fn init_tui() -> TuiHandle {
    let model = Arc::new(Mutex::new(Model::new()));
    let layer = DevenvTuiLayer::new(model.clone());

    TuiHandle { layer, model }
}
