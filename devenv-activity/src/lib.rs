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
//! ## Using the `#[activity]` macro
//!
//! For cleaner instrumentation, use the `#[activity]` attribute macro:
//!
//! ```ignore
//! use devenv_activity::activity;
//!
//! #[activity("Building shell")]
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
mod serde_valuable;
mod stack;
mod timestamp;

// Re-export the activity macro
pub use devenv_activity_macros::activity;

// Re-export for convenience
pub use tracing_subscriber::Registry;

// Core types
pub use activity::{Activity, ActivityType};
pub use events::{
    ActivityEvent, ActivityLevel, ActivityOutcome, Build, Command, EvalOp, Evaluate,
    ExpectedCategory, Fetch, FetchKind, Message, Operation, SetExpected, Task,
};
pub use timestamp::Timestamp;

// Builders
pub use builders::{
    BuildBuilder, CommandBuilder, EvaluateBuilder, FetchBuilder, OperationBuilder, TaskBuilder,
};

// Functions
pub use handle::{ActivityHandle, init};
pub use serde_valuable::SerdeValue;
pub use stack::{
    current_activity_id, current_activity_level, log_to_evaluate, message, message_with_details,
    message_with_parent, op_to_evaluate, set_expected,
};

// Trait
pub use instrument::ActivityInstrument;
