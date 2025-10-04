use devenv_eval_cache::Op;
use devenv_eval_cache::internal_log::{ActivityType, Field, InternalLog, ResultType, Verbosity};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tracing::{debug, debug_span, info, warn};
use tracing_subscriber::registry::LookupSpan;

/// Get the current user operation ID from the active span extensions
fn current_operation_id() -> Option<devenv_tui::OperationId> {
    // Try to get the tracing registry from the current subscriber
    tracing::dispatcher::get_default(|dispatch| {
        if let Some(registry) = dispatch.downcast_ref::<tracing_subscriber::Registry>() {
            let current_span = tracing::Span::current();
            if let Some(id) = current_span.id()
                && let Some(span_ref) = registry.span(&id) {
                    // Check if current span has an operation ID (stored by DevenvLayer)
                    if let Some(op_id) = span_ref.extensions().get::<devenv_tui::OperationId>() {
                        return Some(op_id.clone());
                    }

                    // Walk up parent spans to find a user operation
                    let mut current = span_ref.parent();
                    while let Some(parent) = current {
                        if let Some(op_id) = parent.extensions().get::<devenv_tui::OperationId>() {
                            return Some(op_id.clone());
                        }
                        current = parent.parent();
                    }
                }
        }
        None
    })
}

/// Bridge that converts Nix internal logs to tracing events
pub struct NixLogBridge {
    /// Current active operations and their associated Nix activities
    active_activities: Arc<Mutex<HashMap<u64, NixActivityInfo>>>,
    /// Current parent operation ID for correlating Nix activities
    current_operation_id: Arc<Mutex<Option<devenv_tui::OperationId>>>,
    /// Evaluation tracking state
    evaluation_state: Arc<Mutex<EvaluationState>>,
}

/// State for tracking file evaluations
#[derive(Debug, Default)]
struct EvaluationState {
    /// Total number of files evaluated
    total_files_evaluated: u64,
    /// Recently evaluated files (for batching)
    pending_files: VecDeque<String>,
    /// Last time we sent an evaluation progress event
    last_progress_update: Option<Instant>,
}

/// Information about an active Nix activity
#[derive(Debug, Clone)]
struct NixActivityInfo {
    operation_id: devenv_tui::OperationId,
    activity_type: ActivityType,
}

impl NixLogBridge {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            active_activities: Arc::new(Mutex::new(HashMap::new())),
            current_operation_id: Arc::new(Mutex::new(None)),
            evaluation_state: Arc::new(Mutex::new(EvaluationState::default())),
        })
    }

    /// Set the current operation ID for correlating Nix activities
    pub fn set_current_operation(&self, operation_id: devenv_tui::OperationId) {
        if let Ok(mut current) = self.current_operation_id.lock() {
            *current = Some(operation_id);
        }
    }

    /// Clear the current operation ID
    pub fn clear_current_operation(&self) {
        // First flush any pending evaluation updates before clearing the operation
        // This ensures the operation_id is still available for the flush
        self.flush_evaluation_updates();

        if let Ok(mut current) = self.current_operation_id.lock() {
            *current = None;
        }

        // Also reset the evaluation state for the next operation
        if let Ok(mut state) = self.evaluation_state.lock() {
            state.total_files_evaluated = 0;
            state.pending_files.clear();
            state.last_progress_update = None;
        }
    }

    /// Flush any pending evaluation updates
    fn flush_evaluation_updates(&self) {
        if let Ok(mut state) = self.evaluation_state.lock()
            && !state.pending_files.is_empty() {
                if let Ok(current) = self.current_operation_id.lock() {
                    if let Some(operation_id) = current.as_ref() {
                        let files: Vec<String> = state.pending_files.drain(..).collect();
                        tracing::debug!("Flushing {} pending evaluation files", files.len());

                        // Emit tracing event for evaluation progress
                        let span = debug_span!(
                            target: "devenv.nix.eval",
                            "nix_evaluation_progress",
                            devenv.ui.message = "evaluating Nix files",
                            devenv.ui.type = "eval",
                            devenv.ui.id = %operation_id,
                            devenv.ui.progress.current = state.total_files_evaluated,
                            files = ?files
                        );
                        span.in_scope(|| {
                            info!(devenv_log = true, "Evaluated {} files", files.len());
                        });
                    } else {
                        tracing::warn!(
                            "No operation ID available for flushing {} pending files",
                            state.pending_files.len()
                        );
                    }
                } else {
                    tracing::warn!("Failed to lock operation ID for flushing");
                }
            }
    }

    /// Process a Nix internal log line and emit appropriate tracing events
    pub fn process_log_line(&self, line: &str) {
        if let Some(parse_result) = InternalLog::parse(line) {
            match parse_result {
                Ok(internal_log) => {
                    self.handle_internal_log(internal_log);
                }
                Err(e) => {
                    tracing::debug!("Failed to parse Nix internal log: {} - line: {}", e, line);
                }
            }
        }
    }

    /// Process a parsed InternalLog directly
    pub fn process_internal_log(&self, log: InternalLog) {
        self.handle_internal_log(log);
    }

    /// Process stderr from a pipe, reading line by line and feeding to the bridge
    pub fn process_stderr<R: std::io::Read>(
        &self,
        stderr: R,
        logging: bool,
    ) -> std::io::Result<()> {
        use std::io::{BufRead, BufReader};

        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            let line = line?;

            // Feed line to bridge for structured log processing
            self.process_log_line(&line);

            // Also output to terminal if logging is enabled
            if logging {
                eprintln!("{}", line);
            }
        }

        Ok(())
    }

    /// Handle a parsed InternalLog entry
    fn handle_internal_log(&self, log: InternalLog) {
        let current_op_id = self
            .current_operation_id
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
            .or_else(|| {
                // Fallback: lookup current operation from tracing span context
                current_operation_id()
            });

        if let Some(operation_id) = current_op_id {
            match log {
                InternalLog::Start {
                    id,
                    typ,
                    text,
                    fields,
                    ..
                } => {
                    self.handle_activity_start(operation_id, id, typ, text, fields);
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
                        && let Some((activity_id, _)) = activities
                            .iter()
                            .find(|(_, info)| info.activity_type == ActivityType::Build)
                        {
                            let span = debug_span!(
                                target: "devenv.nix.build",
                                "nix_phase_progress",
                                devenv.ui.message = %phase,
                                devenv.ui.type = "build",
                                devenv.ui.detail = %phase,
                                devenv.ui.id = %operation_id,
                                activity_id = activity_id,
                                phase = %phase
                            );
                            span.in_scope(|| {
                                info!(devenv_log = true, "Build phase: {}", phase);
                            });
                        }
                }
                InternalLog::Msg { level, ref msg, .. } => {
                    // First check if this is a file evaluation message
                    if let Some(op) = Op::from_internal_log(&log)
                        && let Op::EvaluatedFile { source } = op {
                            self.handle_file_evaluation(operation_id.clone(), source);
                            return;
                        }

                    // Handle regular log messages from Nix builds
                    if level <= Verbosity::Warn {
                        match level {
                            Verbosity::Error => tracing::error!(devenv_log = true, "{}", msg),
                            Verbosity::Warn => tracing::warn!(devenv_log = true, "{}", msg),
                            _ => tracing::info!(devenv_log = true, "{}", msg),
                        }
                    }
                }
            }
        }
    }

    /// Handle the start of a Nix activity
    fn handle_activity_start(
        &self,
        operation_id: devenv_tui::OperationId,
        activity_id: u64,
        activity_type: ActivityType,
        text: String,
        fields: Vec<Field>,
    ) {
        // Store activity info for later correlation
        if let Ok(mut activities) = self.active_activities.lock() {
            activities.insert(
                activity_id,
                NixActivityInfo {
                    operation_id: operation_id.clone(),
                    activity_type,
                },
            );
        }

        match activity_type {
            ActivityType::Build => {
                let derivation_path = fields.first()
                    .and_then(|f| match f {
                        Field::String(s) => Some(s.clone()),
                        _ => None,
                    })
                    .unwrap_or_else(|| text.clone());

                let machine = fields.get(1).and_then(|f| match f {
                    Field::String(s) => Some(s.clone()),
                    _ => None,
                });

                let derivation_name = extract_derivation_name(&derivation_path);

                let span = debug_span!(
                    target: "devenv.nix.build",
                    "nix_derivation_start",
                    devenv.ui.message = %derivation_name,
                    devenv.ui.type = "build",
                    devenv.ui.id = %operation_id,
                    activity_id = activity_id,
                    derivation_path = %derivation_path,
                    derivation_name = %derivation_name,
                    machine = ?machine
                );
                span.in_scope(|| {
                    info!(devenv_log = true, "Building {}", derivation_name);
                });
            }
            ActivityType::QueryPathInfo => {
                if let (Some(Field::String(store_path)), Some(Field::String(substituter))) =
                    (fields.first(), fields.get(1))
                {
                    let package_name = extract_package_name(store_path);

                    let span = debug_span!(
                        target: "devenv.nix.query",
                        "nix_query_start",
                        devenv.ui.message = %package_name,
                        devenv.ui.type = "download",
                        devenv.ui.detail = "query",
                        devenv.ui.id = %operation_id,
                        activity_id = activity_id,
                        store_path = %store_path,
                        package_name = %package_name,
                        substituter = %substituter
                    );
                    span.in_scope(|| {
                        info!(devenv_log = true, "Querying {}", package_name);
                    });
                }
            }
            ActivityType::CopyPath => {
                // CopyPath is the actual download activity that shows byte progress
                if let (Some(Field::String(store_path)), Some(Field::String(substituter))) =
                    (fields.first(), fields.get(1))
                {
                    let package_name = extract_package_name(store_path);

                    let span = debug_span!(
                        target: "devenv.nix.download",
                        "nix_download_start",
                        devenv.ui.message = %package_name,
                        devenv.ui.type = "download",
                        devenv.ui.id = %operation_id,
                        activity_id = activity_id,
                        store_path = %store_path,
                        package_name = %package_name,
                        substituter = %substituter
                    );
                    span.in_scope(|| {
                        info!(devenv_log = true, "Downloading {}", package_name);
                    });
                }
            }
            ActivityType::FetchTree => {
                // FetchTree activities show when fetching Git repos, tarballs, etc.
                let span = debug_span!(
                    target: "devenv.nix.fetch",
                    "fetch_tree_start",
                    devenv.ui.message = %text,
                    devenv.ui.type = "download",
                    devenv.ui.detail = "fetch",
                    devenv.ui.id = %operation_id,
                    activity_id = activity_id,
                    message = %text
                );
                span.in_scope(|| {
                    info!(devenv_log = true, "Fetching {}", text);
                });
            }
            _ => {
                // For other activity types, we can add support as needed
                tracing::debug!("Unhandled Nix activity type: {:?}", activity_type);
            }
        }
    }

    /// Handle the stop of a Nix activity
    fn handle_activity_stop(&self, activity_id: u64, success: bool) {
        if let Ok(mut activities) = self.active_activities.lock()
            && let Some(activity_info) = activities.remove(&activity_id) {
                // If this is the last activity, flush any pending evaluation updates
                if activities.is_empty() {
                    self.flush_evaluation_updates();
                }

                let status = if success { "completed" } else { "failed" };

                match activity_info.activity_type {
                    ActivityType::Build => {
                        let span = debug_span!(
                            target: "devenv.nix.build",
                            "nix_derivation_end",
                            devenv.ui.message = "build complete",
                            devenv.ui.type = "build",
                            devenv.ui.detail = status,
                            devenv.ui.id = %activity_info.operation_id,
                            activity_id = activity_id,
                            success = success
                        );
                        span.in_scope(|| {
                            if success {
                                info!(devenv_log = true, "Build completed successfully");
                            } else {
                                warn!(devenv_log = true, "Build failed");
                            }
                        });
                    }
                    ActivityType::CopyPath => {
                        let span = debug_span!(
                            target: "devenv.nix.download",
                            "nix_download_end",
                            devenv.ui.message = "download complete",
                            devenv.ui.type = "download",
                            devenv.ui.detail = status,
                            devenv.ui.id = %activity_info.operation_id,
                            activity_id = activity_id,
                            success = success
                        );
                        span.in_scope(|| {
                            if success {
                                info!(devenv_log = true, "Download completed successfully");
                            } else {
                                warn!(devenv_log = true, "Download failed");
                            }
                        });
                    }
                    ActivityType::QueryPathInfo => {
                        let span = debug_span!(
                            target: "devenv.nix.query",
                            "nix_query_end",
                            devenv.ui.message = "query complete",
                            devenv.ui.type = "download",
                            devenv.ui.detail = status,
                            devenv.ui.id = %activity_info.operation_id,
                            activity_id = activity_id,
                            success = success
                        );
                        span.in_scope(|| {
                            if success {
                                debug!(devenv_log = true, "Query completed successfully");
                            } else {
                                warn!(devenv_log = true, "Query failed");
                            }
                        });
                    }
                    ActivityType::FetchTree => {
                        let span = debug_span!(
                            target: "devenv.nix.fetch",
                            "fetch_tree_end",
                            devenv.ui.message = "fetch complete",
                            devenv.ui.type = "download",
                            devenv.ui.detail = status,
                            devenv.ui.id = %activity_info.operation_id,
                            activity_id = activity_id,
                            success = success
                        );
                        span.in_scope(|| {
                            if success {
                                info!(devenv_log = true, "Fetch completed successfully");
                            } else {
                                warn!(devenv_log = true, "Fetch failed");
                            }
                        });
                    }
                    _ => {}
                }
            }
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
                    if let (
                        Some(Field::Int(done)),
                        Some(Field::Int(expected)),
                        Some(Field::Int(running)),
                        Some(Field::Int(failed)),
                    ) = (fields.first(), fields.get(1), fields.get(2), fields.get(3))
                        && let Ok(activities) = self.active_activities.lock()
                            && let Some(activity_info) = activities.get(&activity_id) {
                                let span = debug_span!(
                                    target: "devenv.nix.progress",
                                    "nix_activity_progress",
                                    devenv.ui.message = "progress update",
                                    devenv.ui.type = "progress",
                                    devenv.ui.id = %activity_info.operation_id,
                                    devenv.ui.progress.current = done,
                                    devenv.ui.progress.total = expected,
                                    activity_id = activity_id,
                                    done = done,
                                    expected = expected,
                                    running = running,
                                    failed = failed
                                );
                                span.in_scope(|| {
                                    debug!(
                                        devenv_log = true,
                                        "Progress: {}/{} done, {} running, {} failed",
                                        done,
                                        expected,
                                        running,
                                        failed
                                    );
                                });
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
                            && let Some(activity_info) = activities.get(&activity_id) {
                                // Only CopyPath activities have byte-based download progress
                                if activity_info.activity_type == ActivityType::CopyPath {
                                    let span = debug_span!(
                                        target: "devenv.nix.download",
                                        "nix_download_progress",
                                        devenv.ui.message = "download progress",
                                        devenv.ui.type = "download",
                                        devenv.ui.id = %activity_info.operation_id,
                                        devenv.ui.download.size_current = downloaded,
                                        devenv.ui.download.size_total = ?total_bytes,
                                        activity_id = activity_id,
                                        bytes_downloaded = downloaded,
                                        total_bytes = ?total_bytes
                                    );
                                    span.in_scope(|| {
                                        if let Some(total) = total_bytes {
                                            let percent =
                                                (*downloaded as f64 / total as f64) * 100.0;
                                            debug!(
                                                devenv_log = true,
                                                "Download progress: {} / {} bytes ({:.1}%)",
                                                downloaded,
                                                total,
                                                percent
                                            );
                                        } else {
                                            debug!(
                                                devenv_log = true,
                                                "Download progress: {} bytes", downloaded
                                            );
                                        }
                                    });
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
                            && activity_info.activity_type == ActivityType::Build {
                                let span = debug_span!(
                                    target: "devenv.nix.build",
                                    "nix_phase_progress",
                                    devenv.ui.message = %phase,
                                    devenv.ui.type = "build",
                                    devenv.ui.detail = %phase,
                                    devenv.ui.id = %activity_info.operation_id,
                                    devenv.ui.build.phase = %phase,
                                    activity_id = activity_id,
                                    phase = %phase
                                );
                                span.in_scope(|| {
                                    info!(devenv_log = true, "Build phase: {}", phase);
                                });
                            }
            }
            ResultType::BuildLogLine => {
                // Handle build log output
                if let Some(Field::String(log_line)) = fields.first()
                    && let Ok(activities) = self.active_activities.lock()
                        && let Some(activity_info) = activities.get(&activity_id) {
                            let span = debug_span!(
                                target: "devenv.nix.build",
                                "build_log",
                                devenv.ui.message = "build log",
                                devenv.ui.type = "build",
                                devenv.ui.id = %activity_info.operation_id,
                                activity_id = activity_id,
                                line = %log_line
                            );
                            span.in_scope(|| {
                                info!(
                                    target: "devenv.ui.log",
                                    devenv_log = true,
                                    devenv_ui_log_stdout = %log_line,
                                    "Build output: {}", log_line
                                );
                            });
                        }
            }
            _ => {
                // Handle other result types as needed
                tracing::debug!("Unhandled Nix result type: {:?}", result_type);
            }
        }
    }

    /// Handle file evaluation events
    fn handle_file_evaluation(
        &self,
        operation_id: devenv_tui::OperationId,
        file_path: std::path::PathBuf,
    ) {
        const BATCH_SIZE: usize = 5; // Reduced from 10 for more responsive updates
        const BATCH_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(100); // Reduced from 200ms

        if let Ok(mut state) = self.evaluation_state.lock() {
            let file_path_str = file_path.display().to_string();

            // If this is the first file, send a start event
            if state.total_files_evaluated == 0 && state.pending_files.is_empty() {
                let span = debug_span!(
                    target: "devenv.nix.eval",
                    "nix_evaluation_start",
                    devenv.ui.message = "evaluating Nix files",
                    devenv.ui.type = "eval",
                    devenv.ui.id = %operation_id,
                    file_path = %file_path_str,
                    total_files_evaluated = 0
                );
                span.in_scope(|| {
                    info!(
                        devenv_log = true,
                        "Starting Nix evaluation: {}", file_path_str
                    );
                });
            }

            // Add to pending files
            state.pending_files.push_back(file_path_str);
            state.total_files_evaluated += 1;

            // Check if we should send a batch update
            let now = Instant::now();
            let should_send = state.pending_files.len() >= BATCH_SIZE
                || (state.last_progress_update.is_some()
                    && now.duration_since(
                        state
                            .last_progress_update
                            .expect("last_progress_update should be Some when is_some() is true"),
                    ) >= BATCH_TIMEOUT);

            if should_send && !state.pending_files.is_empty() {
                let files: Vec<String> = state.pending_files.drain(..).collect();
                let span = debug_span!(
                    target: "devenv.nix.eval",
                    "nix_evaluation_progress",
                    devenv.ui.message = "evaluating Nix files",
                    devenv.ui.type = "eval",
                    devenv.ui.id = %operation_id,
                    devenv.ui.progress.current = state.total_files_evaluated,
                    files = ?files,
                    total_files_evaluated = state.total_files_evaluated
                );
                span.in_scope(|| {
                    info!(
                        devenv_log = true,
                        "Evaluated {} files (total: {})",
                        files.len(),
                        state.total_files_evaluated
                    );
                });
                state.last_progress_update = Some(now);
            } else if state.last_progress_update.is_none() {
                // First file - set the timer
                state.last_progress_update = Some(now);
            }
        }
    }
}

/// Extract a human-readable derivation name from a derivation path
fn extract_derivation_name(derivation_path: &str) -> String {
    // Remove .drv suffix if present
    let path = derivation_path
        .strip_suffix(".drv")
        .unwrap_or(derivation_path);

    // Extract the name part after the hash
    if let Some(dash_pos) = path.rfind('-')
        && let Some(slash_pos) = path[..dash_pos].rfind('/') {
            return path[slash_pos + 1..].to_string();
        }

    // Fallback: just take the filename
    path.split('/').next_back().unwrap_or(path).to_string()
}

/// Extract a human-readable package name from a store path
fn extract_package_name(store_path: &str) -> String {
    // Extract the name part after the hash (format: /nix/store/hash-name)
    if let Some(dash_pos) = store_path.rfind('-')
        && let Some(slash_pos) = store_path[..dash_pos].rfind('/') {
            return store_path[slash_pos + 1..].to_string();
        }

    // Fallback: just take the filename
    store_path
        .split('/')
        .next_back()
        .unwrap_or(store_path)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_derivation_name() {
        assert_eq!(
            extract_derivation_name("/nix/store/abc123-hello-world-1.0.drv"),
            "abc123-hello-world-1.0"
        );
        assert_eq!(
            extract_derivation_name("/nix/store/xyz456-rust-1.70.0"),
            "xyz456-rust-1.70.0"
        );
        assert_eq!(extract_derivation_name("simple-name.drv"), "simple-name");
    }

    #[test]
    fn test_extract_package_name() {
        assert_eq!(
            extract_package_name("/nix/store/abc123-hello-world-1.0"),
            "abc123-hello-world-1.0"
        );
        assert_eq!(
            extract_package_name("/nix/store/xyz456-rust-1.70.0-dev"),
            "xyz456-rust-1.70.0-dev"
        );
        assert_eq!(extract_package_name("simple-name"), "simple-name");
    }
}
