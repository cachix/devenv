//! Trace context propagation for subprocess environments.
//!
//! Provides a hook-based mechanism for injecting OTEL trace context
//! (`TRACEPARENT`, `TRACESTATE`) into subprocess environments without
//! requiring this crate to depend on OpenTelemetry directly.
//!
//! The `devenv` crate registers a propagator when OTLP export is enabled;
//! downstream crates call [`inject_trace_propagation_env()`] to apply them
//! directly to a command without an intermediate collection.

use std::sync::OnceLock;

type PropagatorFn = Box<dyn Fn(&mut dyn FnMut(&str, &str)) + Send + Sync>;

static PROPAGATOR: OnceLock<PropagatorFn> = OnceLock::new();

/// Register a function that extracts the current OTEL trace context as
/// environment variable pairs (e.g. `TRACEPARENT`, `TRACESTATE`).
///
/// Should be called once during tracing initialization when OTLP is enabled.
pub fn register_trace_propagator(f: impl Fn(&mut dyn FnMut(&str, &str)) + Send + Sync + 'static) {
    let _ = PROPAGATOR.set(Box::new(f));
}

/// Inject the current trace context directly into an environment sink.
///
/// The disabled path is one `OnceLock` check with no allocation or dynamic
/// dispatch. The registered propagator is invoked only for OTLP tracing.
#[inline]
pub fn inject_trace_propagation_env(mut inject: impl FnMut(&str, &str)) {
    if let Some(propagator) = PROPAGATOR.get() {
        propagator(&mut inject);
    }
}

/// Return trace context environment variables for the current span.
///
/// Returns an empty vec when no propagator is registered (i.e. OTLP is disabled).
pub fn trace_propagation_env() -> Vec<(String, String)> {
    let mut env = Vec::new();
    inject_trace_propagation_env(|key, value| env.push((key.to_owned(), value.to_owned())));
    env
}
