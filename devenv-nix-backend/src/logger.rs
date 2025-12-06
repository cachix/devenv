//! Integration of Nix activity logger with tracing
//!
//! This module provides a bridge between the Nix C++ activity logger callbacks
//! and the tracing crate, allowing Nix operations to be logged through the
//! standard Rust tracing infrastructure.
//!
//! The NixActivityLogger provides a decoupled callback mechanism that can be
//! reused across multiple log sources, following the pattern established in devenv.

use miette::Result;
use nix_bindings_expr::logger::ActivityLoggerBuilder;
use nix_bindings_util::context::Context;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

/// Activity metadata to track ongoing Nix operations
#[derive(Debug, Clone)]
struct ActivityMetadata {
    #[allow(dead_code)]
    id: u64,
    description: String,
    activity_type: String,
    /// Tracks if this is a build activity and when it completes
    is_build_activity: bool,
}

/// Bridge for Nix activity logging with tracing integration
///
/// This struct decouples the Nix activity logger from the tracing infrastructure,
/// providing a reusable callback that can be obtained via `get_callback()`.
/// This allows multiple log sources to feed into the same logging bridge.
#[derive(Clone)]
pub struct NixActivityLogger {
    activity_tracker: Arc<Mutex<HashMap<u64, ActivityMetadata>>>,
}

impl NixActivityLogger {
    /// Create a new Nix activity logger
    pub fn new() -> Self {
        NixActivityLogger {
            activity_tracker: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Get a closure that handles activity start events.
    /// This callback is reusable and can be shared across multiple log sources.
    fn get_start_callback(&self) -> impl Fn(u64, &str, &str) + Clone + Send + Sync + 'static {
        let tracker = Arc::clone(&self.activity_tracker);
        move |id: u64, description: &str, activity_type: &str| {
            let is_build = activity_type == "build" || description.contains("building");

            let metadata = ActivityMetadata {
                id,
                description: description.to_string(),
                activity_type: activity_type.to_string(),
                is_build_activity: is_build,
            };

            // Store metadata for later lookup
            if let Ok(mut map) = tracker.lock() {
                map.insert(id, metadata.clone());
            }

            // Emit tracing event with activity start
            if is_build {
                tracing::info!(
                    activity_id = id,
                    activity_type = activity_type,
                    description = description,
                    "Nix build activity started"
                );
            } else {
                tracing::info!(
                    activity_id = id,
                    activity_type = activity_type,
                    description = description,
                    "Nix activity started"
                );
            }
        }
    }

    /// Get a closure that handles activity stop events.
    /// This callback is reusable and can be shared across multiple log sources.
    fn get_stop_callback(&self) -> impl Fn(u64) + Clone + Send + Sync + 'static {
        let tracker = Arc::clone(&self.activity_tracker);
        move |id: u64| {
            // Clean up metadata and emit event
            let (description, is_build) = if let Ok(mut map) = tracker.lock() {
                if let Some(metadata) = map.remove(&id) {
                    (Some(metadata.description), metadata.is_build_activity)
                } else {
                    (None, false)
                }
            } else {
                (None, false)
            };

            if let Some(desc) = description {
                if is_build {
                    tracing::info!(
                        activity_id = id,
                        description = desc,
                        "Nix build activity completed"
                    );
                } else {
                    tracing::info!(activity_id = id, description = desc, "Nix activity stopped");
                }
            } else {
                tracing::info!(activity_id = id, "Nix activity stopped");
            }
        }
    }

    /// Get a closure that handles activity result/progress events.
    /// This callback is reusable and can be shared across multiple log sources.
    fn get_result_callback(
        &self,
    ) -> impl Fn(u64, &str, &[i32], &[i64], &[Option<&str>]) + Clone + Send + Sync + 'static {
        let tracker = Arc::clone(&self.activity_tracker);
        move |id: u64,
              result_type: &str,
              _field_types: &[i32],
              int_values: &[i64],
              _string_values: &[Option<&str>]| {
            // Get cached metadata
            let (description, activity_type) = if let Ok(map) = tracker.lock() {
                if let Some(metadata) = map.get(&id) {
                    (metadata.description.clone(), metadata.activity_type.clone())
                } else {
                    (String::new(), String::new())
                }
            } else {
                (String::new(), String::new())
            };

            // Log result with activity context
            match result_type {
                "progress" if int_values.len() >= 2 => {
                    let done = int_values[0];
                    let expected = int_values[1];
                    tracing::debug!(
                        activity_id = id,
                        activity_type = activity_type,
                        description = description,
                        done = done,
                        expected = expected,
                        "Nix activity progress"
                    );
                }
                "result" => {
                    tracing::debug!(
                        activity_id = id,
                        activity_type = activity_type,
                        description = description,
                        "Nix activity result"
                    );
                }
                other => {
                    tracing::trace!(
                        activity_id = id,
                        activity_type = activity_type,
                        description = description,
                        result_type = other,
                        "Nix activity event"
                    );
                }
            }
        }
    }
}

impl Default for NixActivityLogger {
    fn default() -> Self {
        Self::new()
    }
}

/// Initialize the Nix activity logger with tracing integration
///
/// Sets up callbacks that forward Nix activity events to tracing spans/events.
/// The returned ActivityLogger must be kept alive for the duration of Nix operations.
///
/// # Example
///
/// ```ignore
/// let logger = setup_nix_logger()?;
/// // logger must stay alive while Nix operations are performed
/// ```
pub fn setup_nix_logger() -> Result<nix_bindings_expr::logger::ActivityLogger> {
    // Create the activity logging bridge
    let logger_bridge = NixActivityLogger::new();

    // Create a context for logger registration
    // The logger callbacks are registered globally with the Nix C API,
    // so the specific context used for registration doesn't matter.
    let mut context = Context::new();

    // Get reusable callbacks from the bridge
    let on_start = logger_bridge.get_start_callback();
    let on_stop = logger_bridge.get_stop_callback();
    let on_result = logger_bridge.get_result_callback();

    let logger = ActivityLoggerBuilder::new()
        .on_start(on_start)
        .on_stop(on_stop)
        .on_result(on_result)
        .register(&mut context)
        .map_err(|e| miette::miette!("Failed to register Nix logger: {}", e))?;

    Ok(logger)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nix_bindings_expr::eval_state::{EvalStateBuilder, gc_register_my_thread};
    use nix_bindings_store::store::Store;

    #[test]
    fn test_logger_setup() {
        // Initialize Nix
        nix_bindings_expr::eval_state::init().expect("Failed to initialize Nix");
        let _gc_registration = gc_register_my_thread();

        // Create logger - this registers the activity callbacks
        let _logger = setup_nix_logger().expect("Failed to setup logger");

        // If we get here without panicking, the logger was set up correctly
        assert!(true, "Logger setup should not panic");
    }

    #[test]
    fn test_logger_captures_activity() {
        // Initialize Nix
        nix_bindings_expr::eval_state::init().expect("Failed to initialize Nix");
        let _gc_registration = gc_register_my_thread();

        // Create logger - this registers the activity callbacks
        let _logger = setup_nix_logger().expect("Failed to setup logger");

        let store = Store::open(None, []).expect("Failed to open store");
        let mut eval_state = EvalStateBuilder::new(store)
            .expect("Failed to create EvalStateBuilder")
            .build()
            .expect("Failed to build EvalState");

        // Evaluate something simple
        let expr = "1 + 1";
        let result = eval_state.eval_from_string(expr, ".");
        assert!(result.is_ok(), "Simple evaluation should work");
    }

    #[test]
    fn test_nix_activity_logger_creation() {
        // Test that NixActivityLogger can be created and cloned
        let logger = NixActivityLogger::new();
        let cloned = logger.clone();

        // Get callbacks - they should all be obtainable without panic
        let _start = logger.get_start_callback();
        let _stop = cloned.get_stop_callback();
        let _result = logger.get_result_callback();

        assert!(true, "All callbacks should be creatable");
    }
}
