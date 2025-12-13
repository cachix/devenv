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
//! [`with_parent`] to explicitly propagate the parent activity ID across spawn boundaries:
//!
//! ```rust,ignore
//! use devenv_activity::{Activity, with_parent};
//!
//! let activity = Activity::task("parent").start();
//! let parent_id = activity.id();
//!
//! tokio::spawn(async move {
//!     with_parent(parent_id, async {
//!         // Activities created here will have parent_id as their parent
//!         let child = Activity::task("child").start();
//!     }).await;
//! });
//! ```

use std::cell::RefCell;
use std::sync::OnceLock;

use tokio::sync::mpsc;

use crate::Timestamp;
use crate::events::{ActivityEvent, ActivityLevel, Message};

/// Global sender for activity events (installed by ActivityHandle::install())
pub(crate) static ACTIVITY_SENDER: OnceLock<mpsc::Sender<ActivityEvent>> = OnceLock::new();

// Task-local stack for tracking current Activity IDs (for parent detection).
// Using task_local instead of thread_local to support async code where tasks
// can migrate between threads via Tokio's work-stealing scheduler.
tokio::task_local! {
    pub(crate) static ACTIVITY_STACK: RefCell<Vec<u64>>;
}

/// Send an activity event to the registered channel and emit to tracing
pub(crate) fn send_activity_event(event: ActivityEvent) {
    // Emit to tracing for file export - serialize as JSON string
    if let Ok(json) = serde_json::to_string(&event) {
        tracing::trace!(target: "devenv::activity", event = json);
    }

    // Send to channel for TUI (non-blocking)
    if let Some(tx) = ACTIVITY_SENDER.get() {
        let _ = tx.try_send(event);
    }
}

/// Get the activity ID from the current activity stack.
/// Returns None if not in an activity scope or if the stack is empty.
///
/// Use this to capture the current activity ID before crossing thread/task boundaries,
/// then pass it explicitly to activities created in other contexts.
pub fn current_activity_id() -> Option<u64> {
    ACTIVITY_STACK
        .try_with(|stack| stack.borrow().last().copied())
        .ok()
        .flatten()
}

/// Get a clone of the current activity stack.
/// Returns empty vec if not in an activity scope.
pub(crate) fn get_current_stack() -> Vec<u64> {
    ACTIVITY_STACK
        .try_with(|stack| stack.borrow().clone())
        .unwrap_or_default()
}

/// Run a future with the given activity ID as the current parent.
///
/// This is useful for crossing task/spawn boundaries where you need to propagate
/// activity context. Capture the parent ID before spawning, then use this function
/// inside the spawned task.
///
/// # Example
/// ```ignore
/// let parent_id = activity.id();
/// tokio::spawn(async move {
///     with_parent(parent_id, async {
///         // Activities created here will have parent_id as their parent
///         let child = Activity::task("child").start();
///     }).await;
/// });
/// ```
pub async fn with_parent<F, T>(parent_id: u64, f: F) -> T
where
    F: std::future::Future<Output = T>,
{
    let mut stack = get_current_stack();
    stack.push(parent_id);
    ACTIVITY_STACK.scope(RefCell::new(stack), f).await
}

/// Emit a standalone message, associated with the current activity if one exists
pub fn message(level: ActivityLevel, text: impl Into<String>) {
    message_with_details(level, text, None)
}

/// Emit a standalone message with optional details, associated with the current activity if one exists
pub fn message_with_details(
    level: ActivityLevel,
    text: impl Into<String>,
    details: Option<String>,
) {
    let text_str = text.into();
    let parent = current_activity_id();
    send_activity_event(ActivityEvent::Message(Message {
        level,
        text: text_str,
        details,
        parent,
        timestamp: Timestamp::now(),
    }));
}
