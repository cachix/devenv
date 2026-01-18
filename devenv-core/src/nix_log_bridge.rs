//! Bridge that converts Nix logs to the devenv Activity system.
//!
//! This module provides a unified way to process Nix log events from both:
//! - CLI backend: Parses `@nix` JSON lines from stderr
//! - FFI backend: Receives callbacks from Nix C API
//!
//! Both backends convert their input to `InternalLog` and feed it to `NixLogBridge`,
//! ensuring consistent activity tracking and progress reporting.
//!
//! # Eval Activity Lifecycle
//!
//! The bridge uses **lazy activity creation** to avoid creating empty activities
//! when no Nix work actually happens. This is important because some operations
//! (like lock validation) may or may not trigger Nix callbacks depending on
//! whether inputs are cached.
//!
//! ## How It Works
//!
//! 1. Caller captures the parent activity ID from the task-local stack
//! 2. Caller acquires the eval_state lock
//! 3. Caller calls `begin_eval(parent_id)` to register the pending parent
//! 4. **Lazy**: When the first Nix callback arrives, the bridge creates the
//!    eval activity using the stored parent ID
//! 5. Caller calls `end_eval()` when done, which completes the activity (if created)
//!
//! This is handled automatically by `EvalSession` in `devenv-nix-backend`.
//!
//! ## Re-entrancy
//!
//! The bridge supports nested eval sessions. Only the outermost session's parent
//! ID is used, and the activity is only completed when all sessions have ended.
//! This allows methods like `validate_lock_file()` to be called from within
//! operations like `dev_env()` without creating duplicate activities.

use devenv_activity::{
    Activity, ActivityLevel, ExpectedCategory, FetchKind, current_activity_id, message,
    message_with_details, set_expected,
};
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::{Arc, Mutex};
use tracing::{error, trace, warn};

use crate::eval_op::{EvalOp, OpObserver};
use crate::internal_log::{ActivityType, Field, InternalLog, ResultType, Verbosity};

/// State for tracking the current evaluation activity.
///
/// This tracks both the pending parent (for lazy creation) and the nesting depth
/// (for re-entrancy support).
struct EvalActivityState {
    /// Nesting depth - how many EvalSessions are currently active.
    /// Only the outermost session controls activity creation/completion.
    depth: usize,
    /// Parent activity ID captured by the outermost session.
    /// Used when lazily creating the eval activity on first Nix callback.
    pending_parent: Option<u64>,
    /// Name for the evaluation activity (e.g., "Building shell", "Searching packages").
    pending_name: String,
    /// Activity level for the evaluation (Info for user-visible, Debug for internal).
    pending_level: ActivityLevel,
    /// The current evaluation activity, created lazily on first Nix callback.
    current_eval: Option<Activity>,
}

/// Bridge that converts Nix internal logs to tracing events.
///
/// The bridge manages eval activity lifecycle with lazy creation - the activity
/// is only created when the first Nix callback arrives, avoiding empty activities
/// for operations that don't trigger any Nix work.
pub struct NixLogBridge {
    /// Current active operations and their associated Nix activities (Build, Fetch, etc.)
    active_activities: Arc<Mutex<HashMap<u64, NixActivityInfo>>>,
    /// State for the current evaluation activity (lazy creation + re-entrancy)
    eval_state: Mutex<EvalActivityState>,
    /// Observers for file/env operations during eval (used by caching systems)
    observers: Mutex<Vec<Arc<dyn OpObserver>>>,
}

/// Information about an active Nix activity
struct NixActivityInfo {
    activity_type: ActivityType,
    activity: Activity,
}

impl NixLogBridge {
    /// Create a new NixLogBridge.
    ///
    /// The bridge starts with no active evaluation. Call `begin_eval()` before
    /// performing Nix operations to enable activity tracking.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            active_activities: Arc::new(Mutex::new(HashMap::new())),
            eval_state: Mutex::new(EvalActivityState {
                depth: 0,
                pending_parent: None,
                pending_name: String::new(),
                pending_level: ActivityLevel::Info,
                current_eval: None,
            }),
            observers: Mutex::new(Vec::new()),
        })
    }

    /// Add an observer to receive operation notifications during evaluation.
    ///
    /// Observers are notified of file/env operations (EvalOp) as they are parsed
    /// from Nix log messages. This is used by caching systems to track dependencies.
    pub fn add_observer(&self, observer: Arc<dyn OpObserver>) {
        if let Ok(mut guard) = self.observers.lock() {
            guard.push(observer);
        }
    }

    /// Clear all observers after evaluation completes.
    ///
    /// This should be called after evaluation to stop notifying observers
    /// and allow them to be garbage collected.
    pub fn clear_observers(&self) {
        if let Ok(mut guard) = self.observers.lock() {
            guard.clear();
        }
    }

    /// Mark the current evaluation activity as cached.
    ///
    /// Call this when the entire evaluation was served from cache
    /// and no actual Nix evaluation was performed.
    ///
    /// Only works if called within an active eval scope (after `begin_eval`).
    pub fn mark_cached(&self) {
        let state = self.eval_state.lock().expect("eval_state mutex poisoned");

        if let Some(ref eval) = state.current_eval {
            eval.cached();
        }
    }

    /// Begin a new evaluation scope.
    ///
    /// Call this after acquiring the eval_state lock. The `parent_id` should be
    /// captured from the task-local activity stack (`current_activity_id()`)
    /// **before** acquiring the lock.
    ///
    /// The eval activity is created **lazily** when the first Nix callback arrives.
    /// If no callbacks arrive, no activity is created (avoiding empty 0ms activities).
    ///
    /// This method supports re-entrancy: nested calls increment a depth counter,
    /// and only the outermost call's `parent_id` is used.
    pub fn begin_eval(&self, parent_id: Option<u64>, name: String, level: ActivityLevel) {
        let mut state = self.eval_state.lock().expect("eval_state mutex poisoned");

        if state.depth == 0 {
            // Outermost session - store the parent ID, name, and level for lazy activity creation
            state.pending_parent = parent_id;
            state.pending_name = name;
            state.pending_level = level;
            // Clear any stale activity (shouldn't happen, but be safe)
            state.current_eval = None;
        }
        // Nested sessions don't change pending state - outer session's values are used

        state.depth += 1;
    }

    /// End the current evaluation scope.
    ///
    /// Call this when the eval_state lock is released (typically via RAII guard).
    /// This decrements the nesting depth, and when the outermost scope ends,
    /// completes the eval activity (if one was created).
    pub fn end_eval(&self) {
        let mut state = self.eval_state.lock().expect("eval_state mutex poisoned");

        if state.depth == 0 {
            // Unbalanced end_eval call - shouldn't happen but don't panic
            tracing::warn!("end_eval called without matching begin_eval");
            return;
        }

        state.depth -= 1;

        if state.depth == 0 {
            // Outermost scope ended - complete the activity by dropping it
            state.current_eval = None;
            state.pending_parent = None;
            state.pending_name.clear();
            state.pending_level = ActivityLevel::Info;
        }
    }

    /// Ensure an eval activity exists and return its ID.
    ///
    /// When in an eval scope (after `begin_eval`), creates the activity lazily
    /// on first call using the pending name/level. When NOT in an eval scope,
    /// falls back to the task-local activity if present (e.g., from the caching
    /// layer using `.in_activity()`).
    ///
    /// Returns `None` if not in an eval scope and no task-local activity exists.
    fn ensure_eval_activity(&self) -> Option<u64> {
        let mut state = self.eval_state.lock().expect("eval_state mutex poisoned");

        if state.depth == 0 {
            // Not in an eval scope - fall back to task-local activity if present
            // (e.g., from the caching layer using .in_activity())
            drop(state);
            return current_activity_id();
        }

        if state.current_eval.is_none() {
            // First callback in this eval scope - create the activity lazily
            let eval = Activity::evaluate(&state.pending_name)
                .parent(state.pending_parent)
                .level(state.pending_level)
                .start();
            state.current_eval = Some(eval);
        }

        state.current_eval.as_ref().map(|a| a.id())
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
                fields,
                ..
            } => {
                self.handle_activity_start(id, typ, text, fields);
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
                // Extract any input operation from the log for caching
                if let Some(op) = EvalOp::from_internal_log(&log) {
                    // Notify all active observers
                    if let Ok(guard) = self.observers.lock() {
                        for observer in guard.iter() {
                            if observer.is_active() {
                                observer.on_op(op.clone());
                            }
                        }
                    }

                    // Handle file evaluation messages for UI
                    if let EvalOp::EvaluatedFile { source } = op {
                        self.handle_file_evaluation(source);
                        return;
                    }
                }

                // Filter out noise - fast local operations that aren't meaningful to users
                if msg.contains("to the store") || msg.contains("copying path") {
                    return;
                }

                // Handle regular log messages from Nix builds
                // Note: Nix daemon incorrectly labels many routine build messages as
                // Verbosity::Error (e.g., "setting up chroot environment", "executing builder").
                // Only treat Error-level messages as actual errors if they pass is_nix_error()
                // or is_builtin_trace() checks.
                if level == Verbosity::Error {
                    if log.is_nix_error() || log.is_builtin_trace() {
                        let (summary, details) = parse_nix_error(msg);
                        message_with_details(ActivityLevel::Error, summary, details);
                        error!("{msg}");
                    }
                    // Skip falsely-labeled error messages from nix daemon
                } else if level <= Verbosity::Warn {
                    let activity_level = match level {
                        Verbosity::Warn => ActivityLevel::Warn,
                        _ => ActivityLevel::Info,
                    };
                    message(activity_level, msg);
                }
            }
        }
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

    /// Get the parent activity ID for Nix activities.
    ///
    /// This triggers lazy creation of the eval activity if we're in an eval scope.
    /// Returns `None` if not in an eval scope.
    fn get_parent_activity_id(&self) -> Option<u64> {
        self.ensure_eval_activity()
    }

    /// Handle the start of a Nix activity
    fn handle_activity_start(
        &self,
        activity_id: u64,
        activity_type: ActivityType,
        text: String,
        fields: Vec<Field>,
    ) {
        let parent_id = self.get_parent_activity_id();

        match activity_type {
            ActivityType::Build => {
                let derivation_path = fields
                    .first()
                    .and_then(Self::extract_string_field)
                    .unwrap_or_else(|| text.clone());

                let derivation_name = extract_derivation_name(&derivation_path);

                let activity = Activity::build(derivation_name)
                    .id(activity_id)
                    .derivation_path(derivation_path)
                    .parent(parent_id)
                    .start();

                self.insert_activity(activity_id, activity_type, activity);
            }
            ActivityType::BuildWaiting => {
                // Build is queued, waiting for a build slot
                let derivation_path = fields
                    .first()
                    .and_then(Self::extract_string_field)
                    .unwrap_or_else(|| text.clone());

                let derivation_name = extract_derivation_name(&derivation_path);

                let activity = Activity::build(derivation_name)
                    .id(activity_id)
                    .derivation_path(derivation_path)
                    .parent(parent_id)
                    .queue();

                self.insert_activity(activity_id, activity_type, activity);
            }
            ActivityType::QueryPathInfo => {
                if let Some(store_path) = fields.first().and_then(Self::extract_string_field) {
                    let package_name = extract_package_name(&store_path);
                    let substituter = fields.get(1).and_then(Self::extract_string_field);

                    let mut builder = Activity::fetch(FetchKind::Query, package_name)
                        .id(activity_id)
                        .parent(parent_id);
                    if let Some(url) = substituter {
                        builder = builder.url(url);
                    }
                    let activity = builder.start();

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
                        Activity::fetch(FetchKind::Copy, source_path)
                            .id(activity_id)
                            .parent(parent_id)
                            .start()
                    } else if let Some(url) = source_uri {
                        // Remote download from substituter
                        let package_name = extract_package_name(&store_path);
                        Activity::fetch(FetchKind::Download, package_name)
                            .id(activity_id)
                            .parent(parent_id)
                            .url(url)
                            .start()
                    } else {
                        // No source URI - treat as local copy with store path name
                        let package_name = extract_package_name(&store_path);
                        Activity::fetch(FetchKind::Copy, package_name)
                            .id(activity_id)
                            .parent(parent_id)
                            .start()
                    };

                    self.insert_activity(activity_id, activity_type, activity);
                }
            }
            ActivityType::Substitute => {
                // Substituting a store path from cache
                if let Some(store_path) = fields.first().and_then(Self::extract_string_field) {
                    let package_name = extract_package_name(&store_path);
                    let substituter = fields.get(1).and_then(Self::extract_string_field);

                    let mut builder = Activity::fetch(FetchKind::Download, package_name)
                        .id(activity_id)
                        .parent(parent_id);
                    if let Some(url) = substituter {
                        builder = builder.url(url);
                    }
                    let activity = builder.start();

                    self.insert_activity(activity_id, activity_type, activity);
                }
            }
            ActivityType::FetchTree => {
                let activity = Activity::fetch(FetchKind::Tree, text)
                    .id(activity_id)
                    .parent(parent_id)
                    .start();

                self.insert_activity(activity_id, activity_type, activity);
            }
            ActivityType::FileTransfer => {
                let url = fields.first().and_then(Self::extract_string_field);
                let name = url.as_deref().unwrap_or(&text);

                let mut builder = Activity::fetch(FetchKind::Download, name)
                    .id(activity_id)
                    .parent(parent_id);
                if let Some(url) = url {
                    builder = builder.url(url);
                }
                let activity = builder.start();

                self.insert_activity(activity_id, activity_type, activity);
            }
            _ => {
                trace!(
                    activity_type = ?activity_type,
                    activity_id = activity_id,
                    text = text,
                    fields = ?fields,
                    "Unhandled Nix activity type",
                );
            }
        }
    }

    /// Handle the stop of a Nix activity
    fn handle_activity_stop(&self, activity_id: u64, success: bool) {
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
                        activity_info.activity.progress(*done, *expected);
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
                // This announces aggregate expected counts (e.g., "expect 10 downloads").
                if let (Some(Field::Int(activity_type_int)), Some(Field::Int(expected))) =
                    (fields.first(), fields.get(1))
                {
                    // Map Nix ActivityType to ExpectedCategory
                    let category = ActivityType::try_from(*activity_type_int as i32)
                        .ok()
                        .and_then(|at| match at {
                            ActivityType::CopyPath
                            | ActivityType::FileTransfer
                            | ActivityType::Substitute => Some(ExpectedCategory::Download),
                            ActivityType::Build | ActivityType::BuildWaiting => {
                                Some(ExpectedCategory::Build)
                            }
                            _ => None,
                        });

                    if let Some(cat) = category {
                        set_expected(cat, *expected);
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

    /// Handle file evaluation events
    fn handle_file_evaluation(&self, file_path: std::path::PathBuf) {
        // Ensure eval activity exists (lazy creation) and log to it
        self.ensure_eval_activity();

        let state = self.eval_state.lock().expect("eval_state mutex poisoned");

        if let Some(ref eval) = state.current_eval {
            eval.log(file_path.display().to_string());
        }
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

/// Regex for stripping ANSI escape codes
static ANSI_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\x1b\[[0-9;]*m").expect("valid regex"));

/// Strip ANSI escape codes from a string
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
    use super::*;

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
    fn test_strip_ansi_codes() {
        assert_eq!(strip_ansi_codes("\x1b[31;1merror:\x1b[0m"), "error:");
        assert_eq!(strip_ansi_codes("no codes here"), "no codes here");
        assert_eq!(
            strip_ansi_codes("\x1b[34;1mblue\x1b[0m and \x1b[32mgreen\x1b[0m"),
            "blue and green"
        );
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
}
