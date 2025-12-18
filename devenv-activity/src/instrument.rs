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
///
/// Works across spawn boundaries too - the `&Activity` is only used to extract the ID
/// and span at call time, not captured by the returned future:
/// ```ignore
/// let activity = Arc::new(Activity::operation("Building").start());
/// let activity_clone = Arc::clone(&activity);
/// tokio::spawn(move || {
///     async move { /* ... */ }.in_activity(&activity_clone)  // returns 'static future
/// });
/// ```
pub trait ActivityInstrument: Future + Sized {
    /// Instrument this future with the given activity's context.
    ///
    /// The `&Activity` is only used to extract the ID, level and span at call time;
    /// the returned future captures only `Self`, not the `&Activity` reference.
    /// This means when `Self` is `'static`, the returned future is also `'static`.
    fn in_activity(self, activity: &Activity) -> impl Future<Output = Self::Output> + use<Self> {
        let mut stack = get_current_stack();
        stack.push((activity.id(), activity.level()));
        let span = activity.span();

        ACTIVITY_STACK.scope(
            RefCell::new(stack),
            tracing::Instrument::instrument(self, span),
        )
    }
}

impl<F: Future> ActivityInstrument for F {}
