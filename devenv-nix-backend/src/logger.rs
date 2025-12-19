//! Integration of Nix activity logger with the devenv Activity system
//!
//! This module provides a bridge between the Nix C++ activity logger callbacks
//! and the devenv Activity system via NixLogBridge.
//!
//! FFI callbacks receive raw field data which is converted to `InternalLog`
//! and processed by the shared `NixLogBridge` for consistent behavior with
//! the CLI backend.

use devenv_activity::{Activity, current_activity_id};
use devenv_core::nix_log_bridge::{NixLogBridge, activity_type_from_str, result_type_from_str};
use devenv_eval_cache::internal_log::{Field, InternalLog, Verbosity};
use miette::Result;
use nix_bindings_expr::logger::ActivityLoggerBuilder;
use nix_bindings_util::context::Context;
use std::sync::Arc;

/// Initialize the Nix activity logger with Activity system integration
///
/// Sets up callbacks that forward Nix activity events to the devenv Activity system
/// via NixLogBridge. The returned ActivityLogger must be kept alive for the duration
/// of Nix operations.
pub fn setup_nix_logger() -> Result<nix_bindings_expr::logger::ActivityLogger> {
    setup_nix_logger_with_parent(current_activity_id())
}

/// Initialize the Nix activity logger with a specific parent activity ID
///
/// Use this when you need to specify the parent activity explicitly,
/// such as when the logger is created from a different async context.
pub fn setup_nix_logger_with_parent(
    parent_activity_id: Option<u64>,
) -> Result<nix_bindings_expr::logger::ActivityLogger> {
    // Create an evaluation activity with the given parent
    let eval_activity = Activity::evaluate("").parent(parent_activity_id).start();
    let bridge = NixLogBridge::new(eval_activity);

    let mut context = Context::new();

    // Set verbosity to Talkative so we receive "evaluating file" messages
    // These messages are emitted at lvlTalkative (4) and are needed to show
    // the "Evaluating" activity in the UI
    unsafe {
        nix_bindings_bindgen_raw::set_verbosity(
            context.ptr(),
            nix_bindings_bindgen_raw::verbosity_NIX_LVL_TALKATIVE,
        );
    }

    let on_start = create_start_callback(Arc::clone(&bridge));
    let on_stop = create_stop_callback(Arc::clone(&bridge));
    let on_result = create_result_callback(Arc::clone(&bridge));
    let on_log = create_log_callback(Arc::clone(&bridge));

    let logger = ActivityLoggerBuilder::new()
        .on_start(on_start)
        .on_stop(on_stop)
        .on_result(on_result)
        .on_log(on_log)
        .register(&mut context)
        .map_err(|e| miette::miette!("Failed to register Nix logger: {}", e))?;

    Ok(logger)
}

/// Convert raw FFI field arrays to Vec<Field>
fn convert_fields(
    field_types: &[i32],
    int_values: &[i64],
    string_values: &[Option<&str>],
) -> Vec<Field> {
    let mut fields = Vec::with_capacity(field_types.len());

    for (i, &field_type) in field_types.iter().enumerate() {
        match field_type {
            0 => {
                // Int field
                let value = int_values.get(i).copied().unwrap_or(0);
                fields.push(Field::Int(value.max(0) as u64));
            }
            1 => {
                // String field
                if let Some(Some(s)) = string_values.get(i) {
                    fields.push(Field::String(s.to_string()));
                }
            }
            _ => {}
        }
    }

    fields
}

/// Create a callback that handles activity start events from FFI
fn create_start_callback(
    bridge: Arc<NixLogBridge>,
) -> impl Fn(u64, &str, &str, &[i32], &[i64], &[Option<&str>], u64) + Clone + Send + Sync + 'static
{
    move |id: u64,
          description: &str,
          activity_type: &str,
          field_types: &[i32],
          int_values: &[i64],
          string_values: &[Option<&str>],
          parent: u64| {
        let typ = activity_type_from_str(activity_type);
        let fields = convert_fields(field_types, int_values, string_values);

        let log = InternalLog::Start {
            id,
            typ,
            text: description.to_string(),
            fields,
            level: Verbosity::Info,
            parent,
        };

        bridge.process_internal_log(log);
    }
}

/// Create a callback that handles activity stop events from FFI
fn create_stop_callback(bridge: Arc<NixLogBridge>) -> impl Fn(u64) + Clone + Send + Sync + 'static {
    move |id: u64| {
        let log = InternalLog::Stop { id };
        bridge.process_internal_log(log);
    }
}

/// Create a callback that handles activity result/progress events from FFI
fn create_result_callback(
    bridge: Arc<NixLogBridge>,
) -> impl Fn(u64, &str, &[i32], &[i64], &[Option<&str>]) + Clone + Send + Sync + 'static {
    move |id: u64,
          result_type: &str,
          field_types: &[i32],
          int_values: &[i64],
          string_values: &[Option<&str>]| {
        let Some(typ) = result_type_from_str(result_type) else {
            return;
        };

        let fields = convert_fields(field_types, int_values, string_values);
        let log = InternalLog::Result { id, typ, fields };
        bridge.process_internal_log(log);
    }
}

/// Create a callback that handles log messages from FFI
fn create_log_callback(
    bridge: Arc<NixLogBridge>,
) -> impl Fn(i32, &str) + Clone + Send + Sync + 'static {
    move |level: i32, msg: &str| {
        // Convert level to Verbosity
        let verbosity = match level {
            0 => Verbosity::Error,
            1 => Verbosity::Warn,
            2 => Verbosity::Notice,
            3 => Verbosity::Info,
            4 => Verbosity::Talkative,
            5 => Verbosity::Chatty,
            6 => Verbosity::Debug,
            7 => Verbosity::Vomit,
            _ => Verbosity::Info,
        };

        let log = InternalLog::Msg {
            msg: msg.to_string(),
            raw_msg: None,
            level: verbosity,
        };
        bridge.process_internal_log(log);
    }
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
    fn test_convert_fields() {
        // Test with mixed int and string fields
        let field_types = [0, 1, 0];
        let int_values = [42, 0, 100];
        let string_values = [None, Some("/nix/store/abc-foo"), None];

        let fields = convert_fields(&field_types, &int_values, &string_values);

        assert_eq!(fields.len(), 3);
        assert!(matches!(fields[0], Field::Int(42)));
        assert!(matches!(&fields[1], Field::String(s) if s == "/nix/store/abc-foo"));
        assert!(matches!(fields[2], Field::Int(100)));
    }
}
