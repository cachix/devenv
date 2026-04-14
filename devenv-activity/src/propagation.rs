//! Trace context propagation for subprocess environments.
//!
//! Provides a hook-based mechanism for injecting OTEL trace context
//! (`TRACEPARENT`, `TRACESTATE`) into subprocess environments without
//! requiring this crate to depend on OpenTelemetry directly.
//!
//! The `devenv` crate registers a propagator when OTLP export is enabled;
//! downstream crates call [`trace_propagation_env()`] to get the env vars.

use std::sync::OnceLock;

type PropagatorFn = Box<dyn Fn() -> Vec<(String, String)> + Send + Sync>;

static PROPAGATOR: OnceLock<PropagatorFn> = OnceLock::new();

/// Register a function that extracts the current OTEL trace context as
/// environment variable pairs (e.g. `TRACEPARENT`, `TRACESTATE`).
///
/// Should be called once during tracing initialization when OTLP is enabled.
pub fn register_trace_propagator(f: impl Fn() -> Vec<(String, String)> + Send + Sync + 'static) {
    let _ = PROPAGATOR.set(Box::new(f));
}

/// Return trace context environment variables for the current span.
///
/// Returns an empty vec when no propagator is registered (i.e. OTLP is disabled).
pub fn trace_propagation_env() -> Vec<(String, String)> {
    PROPAGATOR.get().map(|f| f()).unwrap_or_default()
}
