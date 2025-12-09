//! Extension trait for instrumenting futures with activity context.

use std::cell::RefCell;
use std::future::Future;

use crate::activity::Activity;
use crate::stack::{ACTIVITY_STACK, get_current_stack};

/// Extension trait for instrumenting futures with activity context.
///
/// This trait provides an `in_activity` method that propagates both:
/// - The activity stack (so nested activities see the activity as their parent)
/// - The tracing span (for tracing instrumentation)
///
/// # Example
/// ```ignore
/// use devenv_activity::{Activity, ActivityInstrument};
///
/// let activity = Activity::operation("Building").start();
/// async {
///     // Nested activities will have `activity` as their parent
///     let child = Activity::task("child").start();
/// }
/// .in_activity(&activity)
/// .await;
/// ```
pub trait ActivityInstrument: Future + Sized {
    /// Instrument this future with the given activity's context.
    fn in_activity(self, activity: &Activity) -> impl Future<Output = Self::Output> {
        let mut stack = get_current_stack();
        stack.push(activity.id());
        let span = activity.span();

        ACTIVITY_STACK.scope(
            RefCell::new(stack),
            tracing::Instrument::instrument(self, span),
        )
    }
}

impl<F: Future> ActivityInstrument for F {}
