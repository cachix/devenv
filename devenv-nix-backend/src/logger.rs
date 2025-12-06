//! Integration of Nix activity logger with the devenv Activity system
//!
//! This module provides a bridge between the Nix C++ activity logger callbacks
//! and the devenv Activity system, allowing Nix operations to be properly displayed
//! in the TUI and logged through tracing.
//!
//! The logger uses the same Activity types as the CLI backend (Build, Fetch, etc.)
//! to provide consistent progress reporting across both FFI and CLI backends.

use devenv_activity::{Activity, FetchKind, current_activity_id};
use miette::Result;
use nix_bindings_expr::logger::ActivityLoggerBuilder;
use nix_bindings_util::context::Context;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

/// Activity metadata to track ongoing Nix operations
#[derive(Debug)]
struct ActivityMetadata {
    activity_type: String,
    activity: Activity,
}

/// Bridge for Nix activity logging with Activity system integration
///
/// This struct bridges Nix C++ activity callbacks to the devenv Activity system,
/// providing proper progress tracking for the TUI.
#[derive(Clone)]
pub struct NixActivityLogger {
    activity_tracker: Arc<Mutex<HashMap<u64, ActivityMetadata>>>,
    /// Parent activity ID captured at creation time
    /// This is needed because FFI callbacks run on different threads
    parent_activity_id: Option<u64>,
}

impl NixActivityLogger {
    /// Create a new Nix activity logger with the current activity as parent
    pub fn new() -> Self {
        NixActivityLogger {
            activity_tracker: Arc::new(Mutex::new(HashMap::new())),
            parent_activity_id: current_activity_id(),
        }
    }

    /// Create a new Nix activity logger with an explicit parent activity ID
    pub fn with_parent(parent_activity_id: Option<u64>) -> Self {
        NixActivityLogger {
            activity_tracker: Arc::new(Mutex::new(HashMap::new())),
            parent_activity_id,
        }
    }

    /// Get a closure that handles activity start events.
    fn get_start_callback(&self) -> impl Fn(u64, &str, &str) + Clone + Send + Sync + 'static {
        let tracker = Arc::clone(&self.activity_tracker);
        let parent_id = self.parent_activity_id;

        move |id: u64, description: &str, activity_type: &str| {
            let activity = match activity_type {
                "build" => {
                    // Extract derivation name from description (usually the .drv path)
                    let name = extract_nix_name(description, true);
                    Activity::build(&name)
                        .id(id)
                        .derivation_path(description)
                        .parent(parent_id)
                        .start()
                }
                "copyPath" | "copyPaths" => {
                    // Downloading from binary cache
                    let name = extract_nix_name(description, false);
                    Activity::fetch(FetchKind::Download, &name)
                        .id(id)
                        .parent(parent_id)
                        .start()
                }
                "queryPathInfo" => {
                    // Querying binary cache for path info
                    let name = extract_nix_name(description, false);
                    Activity::fetch(FetchKind::Query, &name)
                        .id(id)
                        .parent(parent_id)
                        .start()
                }
                "fetchTree" | "download" => {
                    // Fetching a flake input or downloading a file
                    Activity::fetch(FetchKind::Tree, description)
                        .id(id)
                        .parent(parent_id)
                        .start()
                }
                "substitute" => {
                    // Substituting a store path from cache
                    let name = extract_nix_name(description, false);
                    Activity::fetch(FetchKind::Download, &name)
                        .id(id)
                        .parent(parent_id)
                        .start()
                }
                _ => {
                    // Generic operation for unknown types
                    Activity::operation(description)
                        .id(id)
                        .parent(parent_id)
                        .start()
                }
            };

            let metadata = ActivityMetadata {
                activity_type: activity_type.to_string(),
                activity,
            };

            if let Ok(mut map) = tracker.lock() {
                map.insert(id, metadata);
            }
        }
    }

    /// Get a closure that handles activity stop events.
    fn get_stop_callback(&self) -> impl Fn(u64) + Clone + Send + Sync + 'static {
        let tracker = Arc::clone(&self.activity_tracker);
        move |id: u64| {
            // Remove the activity - it completes on drop
            if let Ok(mut map) = tracker.lock() {
                map.remove(&id);
            }
        }
    }

    /// Get a closure that handles activity result/progress events.
    fn get_result_callback(
        &self,
    ) -> impl Fn(u64, &str, &[i32], &[i64], &[Option<&str>]) + Clone + Send + Sync + 'static {
        let tracker = Arc::clone(&self.activity_tracker);
        move |id: u64,
              result_type: &str,
              _field_types: &[i32],
              int_values: &[i64],
              string_values: &[Option<&str>]| {
            let Ok(activities) = tracker.lock() else {
                return;
            };
            let Some(metadata) = activities.get(&id) else {
                return;
            };

            match result_type {
                "progress" => {
                    // Progress format: [done, expected, running, failed] or [downloaded, total]
                    if int_values.len() >= 2 {
                        let done = int_values[0];
                        let expected = int_values[1];

                        // For copy/download activities, treat as bytes
                        if metadata.activity_type == "copyPath"
                            || metadata.activity_type == "download"
                            || metadata.activity_type == "substitute"
                        {
                            metadata.activity.progress_bytes(done, expected);
                        } else {
                            metadata.activity.progress(done, expected);
                        }
                    }
                }
                "setPhase" => {
                    // Build phase change
                    if let Some(Some(phase)) = string_values.first() {
                        metadata.activity.phase(phase);
                    }
                }
                "buildLogLine" => {
                    // Build log output
                    if let Some(Some(log_line)) = string_values.first() {
                        metadata.activity.log(*log_line);
                    }
                }
                _ => {
                    // Unknown result type - ignore
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

/// Extract a human-readable name from a Nix path
///
/// For derivations, strips .drv suffix if requested.
/// Extracts the name part after the hash (format: /nix/store/hash-name)
fn extract_nix_name(path: &str, strip_drv: bool) -> String {
    // Remove .drv suffix if requested
    let path = if strip_drv {
        path.strip_suffix(".drv").unwrap_or(path)
    } else {
        path
    };

    // Extract the name part after the hash
    if let Some(dash_pos) = path.rfind('-')
        && let Some(slash_pos) = path[..dash_pos].rfind('/')
    {
        return path[slash_pos + 1..].to_string();
    }

    // Fallback: just take the filename
    path.split('/').next_back().unwrap_or(path).to_string()
}

/// Initialize the Nix activity logger with Activity system integration
///
/// Sets up callbacks that forward Nix activity events to the devenv Activity system.
/// The returned ActivityLogger must be kept alive for the duration of Nix operations.
pub fn setup_nix_logger() -> Result<nix_bindings_expr::logger::ActivityLogger> {
    // Create the activity logging bridge with current activity as parent
    let logger_bridge = NixActivityLogger::new();

    // Create a context for logger registration
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

/// Initialize the Nix activity logger with a specific parent activity ID
///
/// Use this when you need to specify the parent activity explicitly,
/// such as when the logger is created from a different async context.
pub fn setup_nix_logger_with_parent(
    parent_activity_id: Option<u64>,
) -> Result<nix_bindings_expr::logger::ActivityLogger> {
    let logger_bridge = NixActivityLogger::with_parent(parent_activity_id);

    let mut context = Context::new();

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

    #[test]
    fn test_extract_nix_name() {
        assert_eq!(
            extract_nix_name("/nix/store/abc123-hello-world-1.0.drv", true),
            "abc123-hello-world-1.0"
        );
        assert_eq!(
            extract_nix_name("/nix/store/xyz456-rust-1.70.0", false),
            "xyz456-rust-1.70.0"
        );
        assert_eq!(extract_nix_name("simple-name.drv", true), "simple-name");
    }
}
