pub mod events;
pub mod tracing_interface;
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
#[derive(Clone, Default)]
pub struct TuiHandle {
    pub model: Arc<Mutex<Model>>,
}

impl TuiHandle {
    pub fn init() -> Self {
        Self::default()
    }

    /// Get a clone of the model handle
    pub fn model(&self) -> Arc<Mutex<Model>> {
        self.model.clone()
    }

    pub fn layer(&self) -> DevenvTuiLayer {
        DevenvTuiLayer::new(Arc::clone(&self.model))
    }
}

/// Initialize the TUI system and return handle
///
/// The TUI should be started manually using SubsystemBuilder::new()
pub fn init_tui() -> TuiHandle {
    TuiHandle::init()
}
