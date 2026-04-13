//! Activity tracking system for devenv built on tracing.
//!
//! This crate provides a unified activity tracking system that:
//! - Uses typed events as the single source of truth
//! - Supports multiple consumers via tracing's layer system
//! - Provides automatic context propagation via span hierarchy
//!
//! ## Usage
//!
//! Use the `ActivityInstrument` trait to instrument async code with activities:
//!
//! ```ignore
//! use devenv_activity::{Activity, ActivityInstrument};
//!
//! let activity = Activity::operation("Building").start();
//! async {
//!     // Nested activities will have `activity` as their parent
//! }
//! .in_activity(&activity)
//! .await;
//! ```
//!
//! ## Using the `#[instrument_activity]` macro
//!
//! For cleaner instrumentation, use the `#[instrument_activity]` attribute macro:
//!
//! ```ignore
//! use devenv_activity::instrument_activity;
//!
//! #[instrument_activity("Building shell")]
//! async fn build_shell() -> Result<()> {
//!     // Function body is automatically instrumented
//!     Ok(())
//! }
//! ```

mod activity;
mod builders;
mod events;
mod handle;
mod instrument;
mod propagation;
mod serde_valuable;
mod stack;
mod timestamp;

// Re-export the instrument_activity proc macro
pub use devenv_activity_macros::instrument_activity;

// Re-export for convenience
pub use tracing_subscriber::Registry;

// Core types
pub use activity::{Activity, ActivityRef, ActivityType};
pub use events::{
    ActivityEvent, ActivityLevel, ActivityOutcome, Build, Command, EvalOp, Evaluate,
    ExpectedCategory, Fetch, FetchKind, Message, Operation, Process, ProcessStatus, SetExpected,
    Task, TaskInfo,
};
pub use timestamp::Timestamp;

// Builders and trait
pub use builders::{
    ActivityStart, BuildBuilder, CommandBuilder, EvaluateBuilder, FetchBuilder, OperationBuilder,
    ProcessBuilder, TaskBuilder, next_id,
};

// Functions
pub use handle::{ActivityGuard, ActivityHandle, init};
pub use serde_valuable::SerdeValue;
pub use stack::{
    current_activity_id, current_activity_level, emit_task_hierarchy, log_to_evaluate, log_to_task,
    message, message_with_details, op_to_evaluate, set_expected,
};

// Traits
pub use instrument::ActivityInstrument;

// Trace context propagation
pub use propagation::{register_trace_propagator, trace_propagation_env};
