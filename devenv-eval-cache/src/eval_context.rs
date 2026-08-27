//! Composable state shared by evaluation extensions.

use std::sync::Arc;

use crate::{EvalInputTracker, EvalResourceRegistry};

/// State managers used by evaluation extensions and the eval cache.
///
/// The context is assembled by the application and injected into an evaluator.
/// It deliberately separates resources, which are acquired and replayed, from
/// inputs, which are observed and validated.
#[derive(Clone)]
pub struct EvalContext {
    resources: Arc<EvalResourceRegistry>,
    inputs: Arc<EvalInputTracker>,
}

impl EvalContext {
    pub fn new(resources: Arc<EvalResourceRegistry>, inputs: Arc<EvalInputTracker>) -> Self {
        Self { resources, inputs }
    }

    pub fn resources(&self) -> &Arc<EvalResourceRegistry> {
        &self.resources
    }

    pub fn inputs(&self) -> &Arc<EvalInputTracker> {
        &self.inputs
    }
}
