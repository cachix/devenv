//! Bridge that converts Nix logs to the devenv Activity system.
//!
//! This module provides a unified way to process Nix log events from both:
//! - CLI backend: Parses `@nix` JSON lines from stderr
//! - FFI backend: Receives callbacks from Nix C API
//!
//! Both backends convert their input to `InternalLog` and feed it to `NixLogBridge`,
//! ensuring consistent activity tracking and progress reporting.
//!
//! # Eval Activity Tracking
//!
//! The bridge tracks which Activity evaluation effects should be attached to.
//! The caller owns the Activity and passes its ID and tracing span to
//! `begin_eval_with_span()` when cross-thread trace parenting is needed.
//!
//! ## How It Works
//!
//! 1. Caller creates an Activity (e.g., `Activity::evaluate("Building shell")`)
//! 2. Caller calls `begin_eval_with_span(activity.id(), activity.span())`, which
//!    returns an `EvalActivityGuard`
//! 3. Structured evaluation effects are appended to that activity
//! 4. When the guard is dropped, `end_eval()` is called automatically
//!
//! This guard-based API ensures eval scopes are always properly closed.

use arc_swap::{ArcSwap, ArcSwapOption};
use devenv_activity::{
    Activity, ActivityLevel, ExpectedCategory, FetchKind, append_eval_log, append_eval_op, message,
    message_with_details, set_expected,
};
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tracing::{Span, error, trace, warn};

use crate::eval_op::{EvalOp, OpObserver};
use crate::internal_log::{
    ActivityType, Field, InternalLog, NixMessageKind, ResultType, Verbosity,
};

/// Tracks per-activity expected counts and computes category totals.
///
/// Nix emits absolute expected counts per activity, potentially re-reporting
/// the same value many times. This tracker deduplicates per-activity counts
/// and computes correct totals by summing across all activities per category.
#[derive(Debug, Default)]
struct ExpectedCountTracker {
    counts: HashMap<(u64, ExpectedCategory), u64>,
}

impl ExpectedCountTracker {
    /// Update the expected count for an activity.
    /// Returns `Some(total)` if the category total changed, `None` otherwise.
    #[must_use]
    fn update(
        &mut self,
        activity_id: u64,
        category: ExpectedCategory,
        expected: u64,
    ) -> Option<u64> {
        let key = (activity_id, category);
        let prev = self.counts.insert(key, expected);
        if prev == Some(expected) {
            return None;
        }
        let total = self
            .counts
            .iter()
            .filter(|((_, c), _)| *c == category)
            .map(|(_, v)| v)
            .sum();
        Some(total)
    }

    /// Remove all counts for an activity (called when it stops).
    /// Does not re-emit totals — we don't want the UI count to go down.
    fn remove_activity(&mut self, activity_id: u64) {
        self.counts.retain(|&(id, _), _| id != activity_id);
    }
}

/// Bridge that converts Nix internal logs to tracing events.
///
/// The bridge manages eval activity lifecycle with lazy creation - the activity
/// is only created when the first Nix callback arrives, avoiding empty activities
/// for operations that don't trigger any Nix work.
pub struct NixLogBridge {
    /// Current active operations and their associated Nix activities (Build, Fetch, etc.)
    active_activities: Arc<Mutex<HashMap<u64, NixActivityInfo>>>,
    /// Current evaluation activity ID. Zero means no active evaluation.
    current_eval_id: AtomicU64,
    /// Tracing parent for Nix callbacks that arrive on worker threads.
    current_eval_span: ArcSwapOption<Span>,
    /// Observers for file/env operations during eval (used by caching systems)
    observers: ArcSwap<Vec<Arc<dyn OpObserver>>>,
    /// Error messages to be printed after TUI exits, before entering REPL
    pre_repl_errors: Mutex<Vec<String>>,
    expected_counts: Mutex<ExpectedCountTracker>,
}

/// Information about an active Nix activity
struct NixActivityInfo {
    activity_type: ActivityType,
    activity: Activity,
}

/// Guard that calls `end_eval` when dropped.
///
/// This ensures the eval scope is always closed, even if the code panics.
pub struct EvalActivityGuard<'a> {
    bridge: &'a NixLogBridge,
}

impl Drop for EvalActivityGuard<'_> {
    fn drop(&mut self) {
        self.bridge.end_eval();
    }
}

/// Restores the previous tracing parent for Nix worker callbacks on drop.
///
/// Nix emits callbacks from worker threads, so a thread-local tracing span is
/// insufficient to associate them with the exact operation that triggered the
/// work. This guard temporarily publishes that operation through `ArcSwap`.
pub struct EvalTracingSpanGuard<'a> {
    bridge: &'a NixLogBridge,
    previous: Option<Arc<Span>>,
    installed: Option<Arc<Span>>,
}

impl Drop for EvalTracingSpanGuard<'_> {
    fn drop(&mut self) {
        if self.installed.is_some() {
            // Restore only if this stage is still current. The opening stage
            // creates the eval session inside its body, which intentionally
            // replaces this temporary parent with the longer-lived eval root.
            self.bridge
                .current_eval_span
                .compare_and_swap(&self.installed, self.previous.take());
        }
    }
}

impl NixLogBridge {
    /// Create a new NixLogBridge.
    ///
    /// The bridge starts with no active evaluation. Call `begin_eval()` before
    /// performing Nix operations to enable activity tracking.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            active_activities: Arc::new(Mutex::new(HashMap::new())),
            current_eval_id: AtomicU64::new(0),
            current_eval_span: ArcSwapOption::empty(),
            observers: ArcSwap::from_pointee(Vec::new()),
            pre_repl_errors: Mutex::new(Vec::new()),
            expected_counts: Mutex::new(ExpectedCountTracker::default()),
        })
    }

    /// Store an error message to be printed before entering REPL.
    ///
    /// Error-level log messages are stored here during evaluation and printed
    /// after the TUI exits (before entering the REPL). This ensures errors are
    /// visible to the user even when the TUI was capturing output.
    pub fn store_pre_repl_error(&self, msg: String) {
        if let Ok(mut errors) = self.pre_repl_errors.lock() {
            errors.push(msg);
        }
    }

    /// Take all stored pre-REPL errors, clearing the internal storage.
    ///
    /// Returns the error messages that were stored during evaluation.
    /// These should be printed before entering the REPL.
    pub fn take_pre_repl_errors(&self) -> Vec<String> {
        if let Ok(mut errors) = self.pre_repl_errors.lock() {
            std::mem::take(&mut *errors)
        } else {
            Vec::new()
        }
    }

    /// Add an observer to receive operation notifications during evaluation.
    ///
    /// Observers are notified of file/env operations (EvalOp) as they are parsed
    /// from Nix log messages. This is used by caching systems to track dependencies.
    pub fn add_observer(&self, observer: Arc<dyn OpObserver>) {
        self.observers.rcu(|current| {
            let mut next = (**current).clone();
            next.push(Arc::clone(&observer));
            next
        });
    }

    /// Remove a previously-added observer by `Arc` identity.
    ///
    /// Intended for scoped observers that run alongside long-lived ones
    /// (e.g. a per-eval collector registered for the duration of a concurrent
    /// Nix FFI call). Production code that needs a permanent observer should
    /// register it once at construction and leave it in place.
    pub fn remove_observer(&self, observer: &Arc<dyn OpObserver>) {
        self.observers.rcu(|current| {
            current
                .iter()
                .filter(|candidate| !Arc::ptr_eq(candidate, observer))
                .cloned()
                .collect::<Vec<_>>()
        });
    }

    /// Clear all observers.
    ///
    /// Exposed primarily for tests that need to reset bridge state between
    /// scenarios. Production code registers long-lived observers at
    /// construction and lets them live for the bridge's lifetime.
    pub fn clear_observers(&self) {
        self.observers.store(Arc::new(Vec::new()));
    }

    /// Begin an evaluation scope.
    ///
    /// Returns a guard that calls `end_eval` when dropped.
    /// The caller owns the Activity and controls its lifecycle.
    pub fn begin_eval(&self, activity_id: u64) -> EvalActivityGuard<'_> {
        self.begin_eval_inner(activity_id, None)
    }

    /// Begin an evaluation scope and propagate its tracing span to Nix worker
    /// callbacks. The span is borrowed by callbacks through `ArcSwap`; no lock
    /// or per-callback allocation is required.
    pub fn begin_eval_with_span(&self, activity_id: u64, span: Span) -> EvalActivityGuard<'_> {
        let span = (!span.is_disabled()).then(|| Arc::new(span));
        self.begin_eval_inner(activity_id, span)
    }

    fn begin_eval_inner(&self, activity_id: u64, span: Option<Arc<Span>>) -> EvalActivityGuard<'_> {
        debug_assert_ne!(
            activity_id, 0,
            "activity IDs use zero as the empty sentinel"
        );
        self.current_eval_span.store(span);
        self.current_eval_id.store(activity_id, Ordering::Release);
        // Scope recorded errors to this eval, so the pre-REPL replay shows
        // only this eval's failures, not ones left over from an earlier eval
        // or store init.
        if let Ok(mut errors) = self.pre_repl_errors.lock() {
            errors.clear();
        }
        EvalActivityGuard { bridge: self }
    }

    /// Temporarily use `span` as the tracing parent for Nix callbacks.
    ///
    /// The disabled-tracing path performs only `Span::is_disabled()`. When a
    /// tracing subscriber is active, entering a stage allocates one `Arc` and
    /// performs one atomic pointer swap; callbacks then read the parent without
    /// locking or allocating.
    pub fn enter_eval_tracing_span(&self, span: &Span) -> EvalTracingSpanGuard<'_> {
        if span.is_disabled() {
            return EvalTracingSpanGuard {
                bridge: self,
                previous: None,
                installed: None,
            };
        }

        let installed = Arc::new(span.clone());
        let previous = self.current_eval_span.swap(Some(Arc::clone(&installed)));
        EvalTracingSpanGuard {
            bridge: self,
            previous,
            installed: Some(installed),
        }
    }

    /// End the current evaluation scope (called by EvalActivityGuard on drop).
    fn end_eval(&self) {
        self.current_eval_id.store(0, Ordering::Release);
        self.current_eval_span.store(None);
    }

    /// Construct a callback activity under its native Nix parent when that
    /// parent is represented by an active devenv activity. Root callbacks and
    /// children of unrepresented Nix activities fall back to the eval span.
    ///
    /// The common root path remains lock-free. A native child performs one
    /// short map lookup and clones only its parent's tracing span handle. The
    /// closure retains its own macro expansion site, so source metadata still
    /// points at the callback handler.
    #[inline]
    fn with_activity_parent<T>(
        &self,
        native_parent_id: u64,
        create: impl FnOnce(Option<u64>) -> T,
    ) -> T {
        if native_parent_id != 0 {
            let parent_span = self.active_activities.lock().ok().and_then(|activities| {
                activities
                    .get(&native_parent_id)
                    .map(|info| info.activity.span())
            });
            if let Some(parent_span) = parent_span {
                return parent_span.in_scope(|| create(Some(native_parent_id)));
            }
        }

        let parent_id = self.get_parent_activity_id();
        let parent = self.current_eval_span.load();
        match parent.as_deref() {
            Some(parent) => parent.in_scope(|| create(parent_id)),
            None => create(parent_id),
        }
    }

    /// Get the parent activity ID for Nix activities.
    ///
    /// Returns the current eval activity ID if in an eval scope, otherwise
    /// falls back to the task-local activity stack. This allows downloads
    /// during `apply_cachix_substituters()` (no eval session) to nest under
    /// the current phase activity (e.g., "Configuring shell").
    fn get_parent_activity_id(&self) -> Option<u64> {
        let id = self.current_eval_id.load(Ordering::Acquire);
        (id != 0)
            .then_some(id)
            .or_else(devenv_activity::current_activity_id)
    }

    /// Returns a callback that can be used by any log source.
    /// Both CLI and FFI backends can use this to feed logs to the bridge.
    pub fn get_log_callback(
        self: &Arc<Self>,
    ) -> impl Fn(InternalLog) + Clone + Send + Sync + 'static {
        let bridge = Arc::clone(self);
        move |log: InternalLog| {
            bridge.process_internal_log(log);
        }
    }

    /// Process a Nix internal log line and emit appropriate tracing events
    pub fn process_log_line(&self, line: &str) {
        if let Some(parse_result) = InternalLog::parse(line) {
            match parse_result {
                Ok(internal_log) => {
                    self.process_internal_log(internal_log);
                }
                Err(e) => {
                    warn!("Failed to parse Nix internal log: {} - line: {}", e, line);
                }
            }
        }
    }

    /// Handle a parsed InternalLog entry
    pub fn process_internal_log(&self, log: InternalLog) {
        match log {
            InternalLog::Start {
                id,
                typ,
                text,
                parent,
                fields,
                ..
            } => {
                self.handle_activity_start(id, typ, text, parent, fields);
            }
            InternalLog::Stop { id } => {
                self.handle_activity_stop(id, true);
            }
            InternalLog::Result { id, typ, fields } => {
                self.handle_activity_result(id, typ, fields);
            }
            InternalLog::SetPhase { phase } => {
                // Find the most recent build activity and update its phase
                if let Ok(activities) = self.active_activities.lock()
                    && let Some((_, activity_info)) = activities
                        .iter()
                        .find(|(_, info)| info.activity_type == ActivityType::Build)
                {
                    activity_info.activity.phase(&phase);
                }
            }
            InternalLog::Msg { level, ref msg, .. } => {
                match log.message_kind() {
                    NixMessageKind::Error => {
                        let (summary, details) = parse_nix_error(msg);
                        message_with_details(ActivityLevel::Error, summary, details);
                        error!("{msg}");
                        // Record so the error survives TUI teardown and can be replayed before the REPL.
                        self.store_pre_repl_error(msg.to_string());
                    }
                    NixMessageKind::Trace => {
                        let (summary, details) = parse_nix_error(msg);
                        message_with_details(ActivityLevel::Error, summary, details);
                        error!("{msg}");
                    }
                    NixMessageKind::Warning => {
                        // TODO: Nix warnings need better handling
                        // Nix prints lots of warnings for innocuous things, e.g. ignored settings.
                        let id = self.get_parent_activity_id().unwrap_or(0);
                        append_eval_log(id, msg);
                        warn!("{msg}");
                    }
                    NixMessageKind::Other => {
                        // Not a classified error/warning/trace, so an Error
                        // verbosity here is just mislabeled daemon noise — keep
                        // it quiet at Debug.
                        let activity_level = match level {
                            Verbosity::Error => ActivityLevel::Debug,
                            Verbosity::Warn | Verbosity::Notice => ActivityLevel::Warn,
                            Verbosity::Info => ActivityLevel::Info,
                            Verbosity::Talkative | Verbosity::Chatty | Verbosity::Debug => {
                                ActivityLevel::Debug
                            }
                            Verbosity::Vomit => ActivityLevel::Trace,
                        };
                        if activity_level <= ActivityLevel::Warn {
                            message(activity_level, msg);
                        } else {
                            let id = self.get_parent_activity_id().unwrap_or(0);
                            append_eval_log(id, msg);
                        }
                    }
                }
            }
        }
    }

    /// Process a dependency received from Nix's dedicated one-shot callback.
    pub fn process_eval_effect(&self, kind: &str, subject: &str, detail: Option<&str>) {
        if let Some(op) = EvalOp::from_effect(kind, subject, detail) {
            self.process_eval_op(op);
        }
    }

    fn process_eval_op(&self, op: EvalOp) {
        let observers = self.observers.load();
        for observer in observers.iter() {
            observer.record(op.clone());
        }
        self.op_to_current_eval(op);
    }

    /// Insert an activity into the active activities map
    fn insert_activity(&self, activity_id: u64, activity_type: ActivityType, activity: Activity) {
        if let Ok(mut activities) = self.active_activities.lock() {
            activities.insert(
                activity_id,
                NixActivityInfo {
                    activity_type,
                    activity,
                },
            );
        }
    }

    /// Extract a string value from a Field
    fn extract_string_field(field: &Field) -> Option<String> {
        match field {
            Field::String(s) => Some(s.clone()),
            _ => None,
        }
    }

    /// Handle the start of a Nix activity
    fn handle_activity_start(
        &self,
        activity_id: u64,
        activity_type: ActivityType,
        text: String,
        native_parent_id: u64,
        fields: Vec<Field>,
    ) {
        match activity_type {
            ActivityType::Build => {
                let derivation_path = fields
                    .first()
                    .and_then(Self::extract_string_field)
                    .unwrap_or_else(|| text.clone());

                let derivation_name = extract_derivation_name(&derivation_path);

                let activity = self.with_activity_parent(native_parent_id, |parent_id| {
                    devenv_activity::start!(
                        Activity::build(derivation_name)
                            .id(activity_id)
                            .derivation_path(derivation_path)
                            .parent(parent_id)
                    )
                });

                self.insert_activity(activity_id, activity_type, activity);
            }
            ActivityType::BuildWaiting => {
                // Build is queued, waiting for a build slot
                let derivation_path = fields
                    .first()
                    .and_then(Self::extract_string_field)
                    .unwrap_or_else(|| text.clone());

                let derivation_name = extract_derivation_name(&derivation_path);

                let activity = self.with_activity_parent(native_parent_id, |parent_id| {
                    devenv_activity::queue!(
                        Activity::build(derivation_name)
                            .id(activity_id)
                            .derivation_path(derivation_path)
                            .parent(parent_id)
                    )
                });

                self.insert_activity(activity_id, activity_type, activity);
            }
            ActivityType::QueryPathInfo => {
                if let Some(store_path) = fields.first().and_then(Self::extract_string_field) {
                    let package_name = extract_package_name(&store_path);
                    let substituter = fields.get(1).and_then(Self::extract_string_field);

                    let mut builder =
                        Activity::fetch(FetchKind::Query, package_name).id(activity_id);
                    if let Some(url) = substituter {
                        builder = builder.url(url);
                    }
                    let activity = self.with_activity_parent(native_parent_id, |parent_id| {
                        devenv_activity::start!(builder.parent(parent_id))
                    });

                    self.insert_activity(activity_id, activity_type, activity);
                }
            }
            ActivityType::CopyPath => {
                // CopyPath fields:
                // - Field 0: store path (what's being copied)
                // - Field 1: source store URI
                // - Field 2: destination store URI
                // If field 1 is an absolute path, it's a local copy; otherwise it's a remote download
                if let Some(store_path) = fields.first().and_then(Self::extract_string_field) {
                    let source_uri = fields.get(1).and_then(Self::extract_string_field);

                    let is_local_copy = source_uri.as_ref().is_some_and(|uri| uri.starts_with('/'));

                    let activity = if is_local_copy {
                        // Local copy to the store - use the full source path as the name
                        let source_path = source_uri.as_ref().unwrap();
                        self.with_activity_parent(native_parent_id, |parent_id| {
                            devenv_activity::start!(
                                Activity::fetch(FetchKind::Copy, source_path)
                                    .id(activity_id)
                                    .parent(parent_id)
                            )
                        })
                    } else if let Some(url) = source_uri {
                        // Remote download from substituter
                        let package_name = extract_package_name(&store_path);
                        self.with_activity_parent(native_parent_id, |parent_id| {
                            devenv_activity::start!(
                                Activity::fetch(FetchKind::Download, package_name)
                                    .id(activity_id)
                                    .parent(parent_id)
                                    .url(url)
                            )
                        })
                    } else {
                        // No source URI - treat as local copy with store path name
                        let package_name = extract_package_name(&store_path);
                        self.with_activity_parent(native_parent_id, |parent_id| {
                            devenv_activity::start!(
                                Activity::fetch(FetchKind::Copy, package_name)
                                    .id(activity_id)
                                    .parent(parent_id)
                            )
                        })
                    };

                    self.insert_activity(activity_id, activity_type, activity);
                }
            }
            ActivityType::Substitute => {
                // Substituting a store path from cache
                if let Some(store_path) = fields.first().and_then(Self::extract_string_field) {
                    let package_name = extract_package_name(&store_path);
                    let substituter = fields.get(1).and_then(Self::extract_string_field);

                    let mut builder =
                        Activity::fetch(FetchKind::Download, package_name).id(activity_id);
                    if let Some(url) = substituter {
                        builder = builder.url(url);
                    }
                    let activity = self.with_activity_parent(native_parent_id, |parent_id| {
                        devenv_activity::start!(builder.parent(parent_id))
                    });

                    self.insert_activity(activity_id, activity_type, activity);
                }
            }
            ActivityType::FetchTree => {
                let activity = self.with_activity_parent(native_parent_id, |parent_id| {
                    devenv_activity::start!(
                        Activity::fetch(FetchKind::Tree, text)
                            .id(activity_id)
                            .parent(parent_id)
                    )
                });

                self.insert_activity(activity_id, activity_type, activity);
            }
            ActivityType::FileTransfer => {
                let url = fields.first().and_then(Self::extract_string_field);
                let name = url.as_deref().unwrap_or(&text);

                let mut builder = Activity::fetch(FetchKind::Download, name).id(activity_id);
                if let Some(url) = url {
                    builder = builder.url(url);
                }
                let activity = self.with_activity_parent(native_parent_id, |parent_id| {
                    devenv_activity::start!(builder.parent(parent_id))
                });

                self.insert_activity(activity_id, activity_type, activity);
            }
            _ => {
                trace!(
                    activity_type = ?activity_type,
                    activity_id = activity_id,
                    native_parent_id = native_parent_id,
                    text = text,
                    fields = ?fields,
                    "Unhandled Nix activity type",
                );
            }
        }
    }

    /// Handle the stop of a Nix activity
    fn handle_activity_stop(&self, activity_id: u64, success: bool) {
        if let Ok(mut tracker) = self.expected_counts.lock() {
            tracker.remove_activity(activity_id);
        }

        let Ok(mut activities) = self.active_activities.lock() else {
            return;
        };
        let Some(activity_info) = activities.remove(&activity_id) else {
            return;
        };

        if !success {
            activity_info.activity.fail();
        }
        // Activity completes on drop
    }

    /// Handle activity result messages (like progress updates)
    fn handle_activity_result(
        &self,
        activity_id: u64,
        result_type: ResultType,
        fields: Vec<Field>,
    ) {
        match result_type {
            ResultType::Progress => {
                // Handle generic progress updates with format [done, expected, running, failed]
                if fields.len() >= 4 {
                    if let (Some(Field::Int(done)), Some(Field::Int(expected)), _, _) =
                        (fields.first(), fields.get(1), fields.get(2), fields.get(3))
                        && let Ok(activities) = self.active_activities.lock()
                        && let Some(activity_info) = activities.get(&activity_id)
                    {
                        activity_info.activity.progress(*done, *expected, None);
                    }
                } else if fields.len() >= 2 {
                    // Fallback to download progress format for backward compatibility
                    if let (Some(Field::Int(downloaded)), total_opt) =
                        (fields.first(), fields.get(1))
                    {
                        let total_bytes = match total_opt {
                            Some(Field::Int(total)) => Some(*total),
                            _ => None,
                        };

                        if let Ok(activities) = self.active_activities.lock()
                            && let Some(activity_info) = activities.get(&activity_id)
                        {
                            // Only CopyPath activities have byte-based download progress
                            if activity_info.activity_type == ActivityType::CopyPath {
                                if let Some(total) = total_bytes {
                                    activity_info.activity.progress_bytes(*downloaded, total);
                                } else {
                                    activity_info.activity.progress_indeterminate(*downloaded);
                                }
                            }
                        }
                    }
                }
            }
            ResultType::SetPhase => {
                // Handle build phase changes
                if let Some(Field::String(phase)) = fields.first()
                    && let Ok(activities) = self.active_activities.lock()
                    && let Some(activity_info) = activities.get(&activity_id)
                    && activity_info.activity_type == ActivityType::Build
                {
                    activity_info.activity.phase(phase);
                }
            }
            ResultType::BuildLogLine => {
                // Handle build log output
                if let Some(Field::String(log_line)) = fields.first()
                    && let Ok(activities) = self.active_activities.lock()
                    && let Some(activity_info) = activities.get(&activity_id)
                {
                    activity_info.activity.log(log_line);
                }
            }
            ResultType::SetExpected => {
                // Handle expected count announcements from Nix.
                // fields[0] is the ActivityType (as int), fields[1] is the expected count.
                // Nix emits absolute counts per activity, potentially re-reporting the same
                // value many times. We track per-activity and only emit when the total changes.
                if let (Some(Field::Int(activity_type_int)), Some(Field::Int(expected))) =
                    (fields.first(), fields.get(1))
                {
                    let category = ActivityType::try_from(*activity_type_int as i32)
                        .ok()
                        .and_then(|at| match at {
                            ActivityType::Builds
                            | ActivityType::Build
                            | ActivityType::BuildWaiting => Some(ExpectedCategory::Build),
                            ActivityType::CopyPaths | ActivityType::Substitute => {
                                Some(ExpectedCategory::Download)
                            }
                            // CopyPath/FileTransfer report bytes, not counts
                            _ => None,
                        });

                    if let Some(cat) = category
                        && let Ok(mut tracker) = self.expected_counts.lock()
                        && let Some(total) = tracker.update(activity_id, cat, *expected)
                    {
                        set_expected(cat, total);
                    }
                }
            }
            _ => {
                trace!(
                    result_type = ?result_type,
                    activity_id = activity_id,
                    fields = ?fields,
                    "Unhandled Nix result type",
                );
            }
        }
    }

    /// Emit a structured eval op to the current eval activity, or fall back
    /// to the surrounding task-local activity when no eval scope is active.
    ///
    /// Returns `true` if the op was attached to some activity, `false` if
    /// there is nothing to attach to (caller should fall back to `message()`).
    fn op_to_current_eval(&self, op: EvalOp) -> bool {
        let id = self.current_eval_id.load(Ordering::Acquire);
        let target = (id != 0)
            .then_some(id)
            .or_else(devenv_activity::current_activity_id);

        let Some(id) = target else {
            return false;
        };

        append_eval_op(id, op.into());
        true
    }
}

/// Convert a string activity type (from FFI) to ActivityType enum
pub fn activity_type_from_str(s: &str) -> ActivityType {
    match s {
        "unknown" => ActivityType::Unknown,
        "copy-path" => ActivityType::CopyPath,
        "file-transfer" => ActivityType::FileTransfer,
        "realise" => ActivityType::Realise,
        "copy-paths" => ActivityType::CopyPaths,
        "builds" => ActivityType::Builds,
        "build" => ActivityType::Build,
        "optimise-store" => ActivityType::OptimiseStore,
        "verify-paths" => ActivityType::VerifyPaths,
        "substitute" => ActivityType::Substitute,
        "query-path-info" => ActivityType::QueryPathInfo,
        "post-build-hook" => ActivityType::PostBuildHook,
        "build-waiting" => ActivityType::BuildWaiting,
        "fetch-tree" => ActivityType::FetchTree,
        _ => ActivityType::Unknown,
    }
}

/// Convert a string result type (from FFI) to ResultType enum
pub fn result_type_from_str(s: &str) -> Option<ResultType> {
    match s {
        "fileLinked" | "file-linked" => Some(ResultType::FileLinked),
        "buildLogLine" | "build-log-line" => Some(ResultType::BuildLogLine),
        "untrustedPath" | "untrusted-path" => Some(ResultType::UntrustedPath),
        "corruptedPath" | "corrupted-path" => Some(ResultType::CorruptedPath),
        "setPhase" | "set-phase" => Some(ResultType::SetPhase),
        "progress" => Some(ResultType::Progress),
        "setExpected" | "set-expected" => Some(ResultType::SetExpected),
        "postBuildLogLine" | "post-build-log-line" => Some(ResultType::PostBuildLogLine),
        "fetchStatus" | "fetch-status" => Some(ResultType::FetchStatus),
        _ => None,
    }
}

/// Extract a human-readable name from a Nix path
///
/// For derivations, strips .drv suffix if present.
/// Extracts the name part after the hash (format: /nix/store/hash-name)
/// The hash is always 32 characters, so we find the first dash after position 32
/// from the start of the filename.
fn extract_nix_name(path: &str, strip_drv: bool) -> String {
    // Remove .drv suffix if requested
    let path = if strip_drv {
        path.strip_suffix(".drv").unwrap_or(path)
    } else {
        path
    };

    // Find the filename (part after last /)
    let filename = path.split('/').next_back().unwrap_or(path);

    // Nix store hashes are 32 characters followed by a dash
    // Format: <32-char-hash>-<name>
    if filename.len() > 33 && filename.chars().nth(32) == Some('-') {
        return filename[33..].to_string();
    }

    // Fallback: return the filename as-is
    filename.to_string()
}

/// Extract a human-readable derivation name from a derivation path
pub fn extract_derivation_name(derivation_path: &str) -> String {
    extract_nix_name(derivation_path, true)
}

/// Extract a human-readable package name from a store path
pub fn extract_package_name(store_path: &str) -> String {
    extract_nix_name(store_path, false)
}

/// Regex for stripping ANSI escape codes (color).
static ANSI_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\x1b\[[0-9;]*m").expect("valid regex"));

/// Strip ANSI color codes so the extracted error summary is plain text.
fn strip_ansi_codes(s: &str) -> String {
    ANSI_REGEX.replace_all(s, "").to_string()
}

/// Parse a Nix error message to extract the summary and details.
///
/// Nix errors have the format:
/// ```text
/// error:
///        … stack trace lines starting with ellipsis …
///        error: <actual error message>
/// ```
///
/// Returns (summary, details) where summary is the final error line
/// and details is the full original message (including stack trace).
fn parse_nix_error(msg: &str) -> (String, Option<String>) {
    // Strip ANSI codes for parsing
    let stripped = strip_ansi_codes(msg);

    // Find the last "error:" which contains the actual error
    if let Some(last_error_pos) = stripped.rfind("error:") {
        let summary = stripped[last_error_pos..].trim().to_string();

        // If there's content before the last error, include the full message as details
        let details_part = stripped[..last_error_pos].trim();
        let details = if details_part.is_empty() || details_part == "error:" {
            None
        } else {
            Some(msg.to_string()) // Keep original with ANSI codes for details
        };

        (summary, details)
    } else {
        (msg.to_string(), None)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

    use tracing::field::{Field as TracingField, Visit};
    use tracing::{Subscriber, span};
    use tracing_subscriber::layer::{Context, SubscriberExt};
    use tracing_subscriber::registry::LookupSpan;
    use tracing_subscriber::{Layer, Registry};

    use super::*;

    #[derive(Clone, Default)]
    struct ParentCapture {
        parent: Arc<AtomicU64>,
        stage: Arc<AtomicU64>,
        child: Arc<Mutex<Option<(u64, &'static str, Option<&'static str>)>>>,
        activities: Arc<Mutex<HashMap<u64, (u64, u64)>>>,
    }

    #[derive(Default)]
    struct ActivityIdVisitor(Option<u64>);

    impl Visit for ActivityIdVisitor {
        fn record_debug(&mut self, _field: &TracingField, _value: &dyn std::fmt::Debug) {}

        fn record_u64(&mut self, field: &TracingField, value: u64) {
            if field.name() == "activity_id" {
                self.0 = Some(value);
            }
        }
    }

    impl<S> Layer<S> for ParentCapture
    where
        S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    {
        fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: Context<'_, S>) {
            if attrs.metadata().name() == "eval_parent" {
                self.parent
                    .store(id.clone().into_u64(), AtomicOrdering::Relaxed);
                return;
            }
            if attrs.metadata().name() == "native_stage" {
                self.stage
                    .store(id.clone().into_u64(), AtomicOrdering::Relaxed);
                return;
            }
            if attrs.metadata().target() != "devenv_activity::spans" {
                return;
            }
            let parent_id = attrs
                .parent()
                .map(|parent| parent.clone().into_u64())
                .or_else(|| ctx.lookup_current().map(|parent| parent.id().into_u64()));
            let parent_id = parent_id.unwrap_or_default();
            let mut visitor = ActivityIdVisitor::default();
            attrs.record(&mut visitor);
            if let Some(activity_id) = visitor.0 {
                self.activities
                    .lock()
                    .unwrap()
                    .insert(activity_id, (id.clone().into_u64(), parent_id));
            }
            *self.child.lock().unwrap() = Some((
                parent_id,
                attrs.metadata().target(),
                attrs.metadata().file(),
            ));
        }
    }

    #[test]
    fn eval_span_context_parents_nix_callback_spans_without_mangling_source() {
        let capture = ParentCapture::default();
        let subscriber = Registry::default().with(capture.clone());

        tracing::subscriber::with_default(subscriber, || {
            let parent = tracing::info_span!("eval_parent");
            let bridge = NixLogBridge::new();
            let _guard = bridge.begin_eval_with_span(1, parent);
            bridge.process_internal_log(InternalLog::Start {
                id: 2,
                level: Verbosity::Info,
                typ: ActivityType::FileTransfer,
                text: "download".to_string(),
                parent: 0,
                fields: vec![Field::String("https://example.test/source".to_string())],
            });
            bridge.process_internal_log(InternalLog::Stop { id: 2 });
        });

        let parent = capture.parent.load(AtomicOrdering::Relaxed);
        let child = capture.child.lock().unwrap();
        let (child_parent, target, file) = child.as_ref().expect("callback span was captured");
        assert_eq!(*child_parent, parent);
        assert_eq!(*target, "devenv_activity::spans");
        assert!(file.unwrap().ends_with("src/nix_log_bridge.rs"));
    }

    #[test]
    fn native_nix_activity_parent_is_preserved_in_trace_hierarchy() {
        let capture = ParentCapture::default();
        let subscriber = Registry::default().with(capture.clone());

        tracing::subscriber::with_default(subscriber, || {
            let eval_parent = tracing::info_span!("eval_parent");
            let bridge = NixLogBridge::new();
            let _guard = bridge.begin_eval_with_span(1, eval_parent);

            bridge.process_internal_log(InternalLog::Start {
                id: 2,
                level: Verbosity::Info,
                typ: ActivityType::FileTransfer,
                text: "parent download".to_string(),
                parent: 0,
                fields: vec![Field::String("https://example.test/parent".to_string())],
            });
            bridge.process_internal_log(InternalLog::Start {
                id: 3,
                level: Verbosity::Info,
                typ: ActivityType::FileTransfer,
                text: "child download".to_string(),
                parent: 2,
                fields: vec![Field::String("https://example.test/child".to_string())],
            });
            bridge.process_internal_log(InternalLog::Stop { id: 3 });
            bridge.process_internal_log(InternalLog::Stop { id: 2 });
        });

        let eval_parent = capture.parent.load(AtomicOrdering::Relaxed);
        let activities = capture.activities.lock().unwrap();
        let (native_parent_span, root_parent) = activities[&2];
        let (_, child_parent) = activities[&3];
        assert_eq!(root_parent, eval_parent);
        assert_eq!(child_parent, native_parent_span);
    }

    #[test]
    fn scoped_eval_span_parents_worker_thread_callbacks_to_the_exact_stage() {
        let capture = ParentCapture::default();
        let subscriber = Registry::default().with(capture.clone());

        tracing::subscriber::with_default(subscriber, || {
            let eval_parent = tracing::info_span!("eval_parent");
            let bridge = NixLogBridge::new();
            let _eval_guard = bridge.begin_eval_with_span(1, eval_parent);
            let stage = tracing::info_span!("native_stage");
            let _stage_guard = bridge.enter_eval_tracing_span(&stage);

            let worker_bridge = Arc::clone(&bridge);
            let dispatch = tracing::dispatcher::get_default(Clone::clone);
            std::thread::spawn(move || {
                tracing::dispatcher::with_default(&dispatch, || {
                    worker_bridge.process_internal_log(InternalLog::Start {
                        id: 2,
                        level: Verbosity::Info,
                        typ: ActivityType::FileTransfer,
                        text: "download".to_string(),
                        parent: 0,
                        fields: vec![Field::String("https://example.test/source".to_string())],
                    });
                    worker_bridge.process_internal_log(InternalLog::Stop { id: 2 });
                });
            })
            .join()
            .unwrap();
        });

        let stage = capture.stage.load(AtomicOrdering::Relaxed);
        let activities = capture.activities.lock().unwrap();
        assert_eq!(activities[&2].1, stage);
    }

    #[test]
    fn opening_stage_does_not_overwrite_the_eval_parent_it_creates() {
        let capture = ParentCapture::default();
        let subscriber = Registry::default().with(capture.clone());

        tracing::subscriber::with_default(subscriber, || {
            let bridge = NixLogBridge::new();
            let stage = tracing::info_span!("native_stage");
            let stage_guard = bridge.enter_eval_tracing_span(&stage);

            let eval_parent = tracing::info_span!("eval_parent");
            let _eval_guard = bridge.begin_eval_with_span(1, eval_parent);
            drop(stage_guard);

            bridge.process_internal_log(InternalLog::Start {
                id: 2,
                level: Verbosity::Info,
                typ: ActivityType::FileTransfer,
                text: "download".to_string(),
                parent: 0,
                fields: vec![Field::String("https://example.test/source".to_string())],
            });
            bridge.process_internal_log(InternalLog::Stop { id: 2 });
        });

        let eval_parent = capture.parent.load(AtomicOrdering::Relaxed);
        let activities = capture.activities.lock().unwrap();
        assert_eq!(activities[&2].1, eval_parent);
    }

    /// An `Error`-level `Msg` — the verbosity real errors and mislabeled
    /// daemon lines share.
    fn error_level_msg(msg: &str) -> InternalLog {
        InternalLog::Msg {
            level: Verbosity::Error,
            msg: msg.to_string(),
            raw_msg: None,
        }
    }

    #[test]
    fn process_internal_log_records_real_errors() {
        let bridge = NixLogBridge::new();
        let _guard = bridge.begin_eval(1);

        let msg = "\u{1b}[31;1merror:\u{1b}[0m syntax error, unexpected '}'";
        bridge.process_internal_log(error_level_msg(msg));

        assert_eq!(bridge.take_pre_repl_errors(), vec![msg.to_string()]);
    }

    #[test]
    fn process_internal_log_does_not_record_mislabeled_warnings() {
        let bridge = NixLogBridge::new();
        let _guard = bridge.begin_eval(1);

        // A restricted-settings notice: a warning the daemon forwards at Error
        // level, in magenta. It must not be recorded as the evaluation error.
        bridge.process_internal_log(error_level_msg(
            "\u{1b}[35;1mwarning:\u{1b}[0m ignoring the client-specified setting \
             'trusted-public-keys', because it is a restricted setting and you \
             are not a trusted user",
        ));

        assert!(bridge.take_pre_repl_errors().is_empty());
    }

    #[test]
    fn begin_eval_clears_stale_pre_repl_errors() {
        let bridge = NixLogBridge::new();
        bridge.store_pre_repl_error("error: stale message from store init".to_string());

        let _guard = bridge.begin_eval(1);

        assert!(bridge.take_pre_repl_errors().is_empty());
    }

    #[test]
    fn test_strip_ansi_codes() {
        assert_eq!(strip_ansi_codes("\x1b[31;1merror:\x1b[0m"), "error:");
        assert_eq!(strip_ansi_codes("no codes here"), "no codes here");
        assert_eq!(
            strip_ansi_codes("\x1b[34;1mblue\x1b[0m and \x1b[32mgreen\x1b[0m"),
            "blue and green"
        );
    }

    #[test]
    fn test_extract_derivation_name() {
        // Real Nix store path with 32-char hash
        assert_eq!(
            extract_derivation_name(
                "/nix/store/kaa3d6q05ipkwdk36vbv8acni8n0g57d-hello-world-1.0.drv"
            ),
            "hello-world-1.0"
        );
        assert_eq!(
            extract_derivation_name("/nix/store/abcdefghijklmnopqrstuvwxyz012345-rust-1.70.0.drv"),
            "rust-1.70.0"
        );
        // Short paths without proper hash format are returned as-is
        assert_eq!(extract_derivation_name("simple-name.drv"), "simple-name");
    }

    #[test]
    fn test_extract_package_name() {
        // Real Nix store path with 32-char hash - hash should be stripped
        assert_eq!(
            extract_package_name("/nix/store/kaa3d6q05ipkwdk36vbv8acni8n0g57d-devenv-shell-env"),
            "devenv-shell-env"
        );
        assert_eq!(
            extract_package_name("/nix/store/abcdefghijklmnopqrstuvwxyz012345-rust-1.70.0-dev"),
            "rust-1.70.0-dev"
        );
        // Short paths without proper hash format are returned as-is
        assert_eq!(extract_package_name("simple-name"), "simple-name");
    }

    #[test]
    fn test_activity_type_from_str() {
        assert_eq!(activity_type_from_str("build"), ActivityType::Build);
        assert_eq!(
            activity_type_from_str("fetch-tree"),
            ActivityType::FetchTree
        );
        assert_eq!(
            activity_type_from_str("substitute"),
            ActivityType::Substitute
        );
        assert_eq!(activity_type_from_str("copy-path"), ActivityType::CopyPath);
        assert_eq!(
            activity_type_from_str("unknown-type"),
            ActivityType::Unknown
        );
    }

    #[test]
    fn test_result_type_from_str() {
        assert_eq!(result_type_from_str("progress"), Some(ResultType::Progress));
        assert_eq!(result_type_from_str("setPhase"), Some(ResultType::SetPhase));
        assert_eq!(
            result_type_from_str("set-phase"),
            Some(ResultType::SetPhase)
        );
        assert_eq!(
            result_type_from_str("buildLogLine"),
            Some(ResultType::BuildLogLine)
        );
        assert_eq!(result_type_from_str("unknown"), None);
    }

    #[test]
    fn test_parse_nix_error_simple() {
        // Simple error without stack trace
        let (summary, details) = parse_nix_error("error: attribute 'foo' not found");
        assert_eq!(summary, "error: attribute 'foo' not found");
        assert!(details.is_none());
    }

    #[test]
    fn test_parse_nix_error_with_stack_trace() {
        // Error with stack trace (like real Nix output)
        let msg = "error:\n       … while evaluating\n         at file.nix:1:1\n\n       error: undefined variable 'pkgs'";
        let (summary, details) = parse_nix_error(msg);
        assert_eq!(summary, "error: undefined variable 'pkgs'");
        assert!(details.is_some());
        assert_eq!(details.unwrap(), msg); // Original message preserved
    }

    #[test]
    fn test_parse_nix_error_with_ansi() {
        // Error with ANSI codes (like real Nix output)
        let msg = "\x1b[31;1merror:\x1b[0m\n       … stack trace\n\n       \x1b[31;1merror:\x1b[0m actual error message";
        let (summary, details) = parse_nix_error(msg);
        assert_eq!(summary, "error: actual error message");
        assert!(details.is_some());
    }

    #[test]
    fn test_parse_nix_error_only_error_prefix() {
        // Just "error:" followed by the actual message on same line
        let (summary, details) = parse_nix_error("error: something went wrong");
        assert_eq!(summary, "error: something went wrong");
        assert!(details.is_none());
    }

    #[test]
    fn test_expected_count_single_activity() {
        let mut tracker = ExpectedCountTracker::default();

        // First report: activity 1 expects 5 downloads
        assert_eq!(tracker.update(1, ExpectedCategory::Download, 5), Some(5),);

        // Same value again: no change
        assert_eq!(tracker.update(1, ExpectedCategory::Download, 5), None,);

        // Updated count: activity 1 now expects 10 downloads
        assert_eq!(tracker.update(1, ExpectedCategory::Download, 10), Some(10),);
    }

    #[test]
    fn test_expected_count_multiple_activities_same_category() {
        let mut tracker = ExpectedCountTracker::default();

        // Activity 1 expects 5 downloads
        assert_eq!(tracker.update(1, ExpectedCategory::Download, 5), Some(5),);

        // Activity 2 expects 3 downloads — total is 8
        assert_eq!(tracker.update(2, ExpectedCategory::Download, 3), Some(8),);

        // Activity 1 re-reports 5 — no change
        assert_eq!(tracker.update(1, ExpectedCategory::Download, 5), None,);

        // Activity 2 updates to 7 — total is 12
        assert_eq!(tracker.update(2, ExpectedCategory::Download, 7), Some(12),);
    }

    #[test]
    fn test_expected_count_independent_categories() {
        let mut tracker = ExpectedCountTracker::default();

        // Builds and downloads tracked independently
        assert_eq!(tracker.update(1, ExpectedCategory::Build, 3), Some(3),);
        assert_eq!(tracker.update(1, ExpectedCategory::Download, 10), Some(10),);
        assert_eq!(tracker.update(2, ExpectedCategory::Build, 2), Some(5),);

        // Download total should still be 10
        assert_eq!(tracker.update(1, ExpectedCategory::Download, 10), None,);
    }

    #[test]
    fn test_expected_count_remove_activity() {
        let mut tracker = ExpectedCountTracker::default();

        let _ = tracker.update(1, ExpectedCategory::Download, 5);
        let _ = tracker.update(2, ExpectedCategory::Download, 3);

        // Remove activity 1 — only activity 2 remains
        tracker.remove_activity(1);

        // Activity 3 reports 2 downloads — total is 3 + 2 = 5 (activity 1 is gone)
        assert_eq!(tracker.update(3, ExpectedCategory::Download, 2), Some(5),);
    }

    #[test]
    fn test_expected_count_remove_cleans_all_categories() {
        let mut tracker = ExpectedCountTracker::default();

        let _ = tracker.update(1, ExpectedCategory::Build, 3);
        let _ = tracker.update(1, ExpectedCategory::Download, 5);

        tracker.remove_activity(1);

        // Both categories should start fresh
        assert_eq!(tracker.update(2, ExpectedCategory::Build, 1), Some(1),);
        assert_eq!(tracker.update(2, ExpectedCategory::Download, 1), Some(1),);
    }

    /// Helper: create a mock observer that records ops in a shared Vec.
    struct MockObserver {
        ops: Mutex<Vec<EvalOp>>,
    }

    impl MockObserver {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                ops: Mutex::new(Vec::new()),
            })
        }

        fn collected_ops(&self) -> Vec<EvalOp> {
            self.ops.lock().unwrap().clone()
        }
    }

    impl OpObserver for MockObserver {
        fn record(&self, op: EvalOp) {
            self.ops.lock().unwrap().push(op);
        }
    }

    #[test]
    fn test_add_observer_receives_dispatched_ops() {
        let bridge = NixLogBridge::new();
        let observer = MockObserver::new();
        bridge.add_observer(observer.clone());

        bridge.process_eval_effect("evaluated-file", "/tmp/default.nix", Some("uncached"));

        assert_eq!(observer.collected_ops().len(), 1);
        assert_eq!(
            observer.collected_ops()[0],
            EvalOp::EvaluatedFile {
                source: "/tmp/default.nix".into(),
                cached: false,
            }
        );
    }

    #[test]
    fn test_multiple_observers_all_receive_ops() {
        let bridge = NixLogBridge::new();
        let obs1 = MockObserver::new();
        let obs2 = MockObserver::new();
        bridge.add_observer(obs1.clone());
        bridge.add_observer(obs2.clone());

        bridge.process_eval_effect("evaluated-file", "/tmp/default.nix", Some("uncached"));

        assert_eq!(obs1.collected_ops().len(), 1);
        assert_eq!(obs2.collected_ops().len(), 1);
    }

    #[test]
    fn test_clear_observers_drops_all() {
        let bridge = NixLogBridge::new();
        let observer = MockObserver::new();
        bridge.add_observer(observer.clone());
        bridge.clear_observers();

        bridge.process_eval_effect("evaluated-file", "/tmp/default.nix", Some("uncached"));

        assert_eq!(observer.collected_ops().len(), 0);
    }
}
