//! Composable state shared by evaluation extensions.

use std::sync::Arc;

use crate::{EvalInputManager, ResourceManager};

/// State managers used by evaluation extensions and the eval cache.
///
/// The context is assembled by the application and injected into an evaluator.
/// It deliberately separates resources, which are acquired and replayed, from
/// inputs, which are observed and validated.
#[derive(Clone)]
pub struct EvalContext {
    resources: Arc<ResourceManager>,
    inputs: Arc<EvalInputManager>,
}

impl EvalContext {
    pub fn new(resources: Arc<ResourceManager>, inputs: Arc<EvalInputManager>) -> Self {
        Self { resources, inputs }
    }

    pub fn resources(&self) -> &Arc<ResourceManager> {
        &self.resources
    }

    pub fn inputs(&self) -> &Arc<EvalInputManager> {
        &self.inputs
    }
}
