//! Task-local activity stack for parent tracking and event dispatch.
//!
//! This module provides a task-local stack that tracks the current activity hierarchy.
//! Activities use this stack to automatically determine their parent when starting,
//! enabling proper parent-child relationships in the activity tree.
//!
//! # Task-Local vs Thread-Local
//!
//! The stack uses Tokio's `task_local!` instead of `thread_local!` to properly support
//! async code where tasks can migrate between threads via Tokio's work-stealing scheduler.
//!
//! # Cross-Spawn Propagation
//!
//! When spawning new tasks, activity context is not automatically propagated. Use
//! [`ActivityInstrument::in_activity`](crate::ActivityInstrument::in_activity) to propagate
//! activity context across spawn boundaries:
//!
//! ```rust,ignore
//! use devenv_activity::{Activity, ActivityInstrument};
//!
//! let activity = Arc::new(Activity::task("parent").start());
//! let activity_clone = Arc::clone(&activity);
//!
//! tokio::spawn(move || {
//!     async move {
//!         // Activities created here will have `activity` as their parent
//!         let child = Activity::task("child").start();
//!     }.in_activity(&activity_clone)
//! });
//! ```

use std::cell::RefCell;
use std::sync::OnceLock;
use tokio::sync::mpsc;
use valuable::Valuable;

use crate::Timestamp;
use crate::builders::next_id;
use crate::events::{ActivityEvent, ActivityLevel, ExpectedCategory, Message, SetExpected};
use crate::serde_valuable::SerdeValue;

/// Global sender for activity events (installed by ActivityHandle::install())
pub(crate) static ACTIVITY_SENDER: OnceLock<mpsc::UnboundedSender<ActivityEvent>> = OnceLock::new();

// Task-local stack for tracking current Activity IDs and levels (for parent detection and level inheritance).
// Using task_local instead of thread_local to support async code where tasks
// can migrate between threads via Tokio's work-stealing scheduler.
tokio::task_local! {
    pub(crate) static ACTIVITY_STACK: RefCell<Vec<(u64, ActivityLevel)>>;
}

/// Send an activity event to the registered channel and emit to tracing
pub(crate) fn send_activity_event(event: ActivityEvent) {
    // Emit to tracing for file export - serialize via serde to respect rename attributes
    if let Ok(serde_value) = SerdeValue::from_serialize(&event) {
        tracing::trace!(target: "devenv::activity", event = serde_value.as_value());
    }

    // Send to channel for TUI
    if let Some(tx) = ACTIVITY_SENDER.get() {
        let _ = tx.send(event);
    }
}

/// Get the activity ID from the current activity stack.
/// Returns None if not in an activity scope or if the stack is empty.
///
/// Use this to capture the current activity ID before crossing thread/task boundaries,
/// then pass it explicitly to activities created in other contexts.
pub fn current_activity_id() -> Option<u64> {
    ACTIVITY_STACK
        .try_with(|stack| stack.borrow().last().map(|(id, _)| *id))
        .ok()
        .flatten()
}

/// Get the activity level from the current activity stack.
/// Returns None if not in an activity scope or if the stack is empty.
///
/// Child activities use this to inherit their parent's level by default.
pub fn current_activity_level() -> Option<ActivityLevel> {
    ACTIVITY_STACK
        .try_with(|stack| stack.borrow().last().map(|(_, level)| *level))
        .ok()
        .flatten()
}

/// Get a clone of the current activity stack.
/// Returns empty vec if not in an activity scope.
pub(crate) fn get_current_stack() -> Vec<(u64, ActivityLevel)> {
    ACTIVITY_STACK
        .try_with(|stack| stack.borrow().clone())
        .unwrap_or_default()
}

/// Emit a standalone message, associated with the current activity if one exists.
pub fn message(level: ActivityLevel, text: impl Into<String>) {
    message_with_details(level, text, None)
}

/// Emit a standalone message with optional details, associated with the current activity if one exists.
pub fn message_with_details(
    level: ActivityLevel,
    text: impl Into<String>,
    details: Option<String>,
) {
    let parent = current_activity_id();
    send_activity_event(ActivityEvent::Message(Message {
        id: next_id(),
        level,
        text: text.into(),
        details,
        parent,
        timestamp: Timestamp::now(),
    }));
}

/// Emit a SetExpected event to announce aggregate expected counts.
/// This is used by Nix to announce how many items/bytes are expected
/// before individual activities start.
pub fn set_expected(category: ExpectedCategory, expected: u64) {
    send_activity_event(ActivityEvent::SetExpected(SetExpected {
        category,
        expected,
        timestamp: Timestamp::now(),
    }));
}

/// Log a line to an Evaluate activity by ID.
///
/// Use this when you have the activity ID but not the Activity object,
/// such as when logging from FFI callbacks where the Activity is owned elsewhere.
pub fn log_to_evaluate(id: u64, line: impl Into<String>) {
    use crate::events::Evaluate;

    send_activity_event(ActivityEvent::Evaluate(Evaluate::Log {
        id,
        line: line.into(),
        timestamp: Timestamp::now(),
    }));
}
