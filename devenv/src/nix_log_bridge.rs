use devenv_eval_cache::Op;
use devenv_eval_cache::internal_log::{ActivityType, Field, InternalLog, ResultType, Verbosity};
use devenv_tui::tracing_interface::{
    nix_fields, operation_fields, operation_types, progress_events, status_events,
};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tracing::{Span, debug, error, info, info_span, trace, warn};

/// Simple operation ID type for correlating Nix activities
pub type OperationId = String;

/// Bridge that converts Nix internal logs to tracing events
pub struct NixLogBridge {
    /// Current active operations and their associated Nix activities
    active_activities: Arc<Mutex<HashMap<u64, NixActivityInfo>>>,
    /// Current parent operation ID for correlating Nix activities
    current_operation_id: Arc<Mutex<Option<OperationId>>>,
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
    /// The span tracking the entire evaluation operation
    span: Option<Span>,
}

/// Information about an active Nix activity
#[derive(Debug)]
struct NixActivityInfo {
    #[allow(dead_code)] // Kept for future activity correlation
    operation_id: OperationId,
    activity_type: ActivityType,
    span: Span,
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
    pub fn set_current_operation(&self, operation_id: OperationId) {
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
            state.span = None; // Drop the span to end evaluation timing
        }
    }

    /// Flush any pending evaluation updates
    fn flush_evaluation_updates(&self) {
        let Ok(mut state) = self.evaluation_state.lock() else {
            warn!("Failed to lock evaluation state for flushing");
            return;
        };

        if state.pending_files.is_empty() {
            return;
        }

        let Ok(current) = self.current_operation_id.lock() else {
            warn!("Failed to lock operation ID for flushing");
            return;
        };

        if current.is_none() {
            warn!(
                "No operation ID available for flushing {} pending files",
                state.pending_files.len()
            );
            return;
        }

        let files: Vec<String> = state.pending_files.drain(..).collect();
        trace!("Flushing {} pending evaluation files", files.len());

        // Emit tracing event for evaluation progress using stored span
        if let Some(ref span) = state.span {
            span.in_scope(|| {
                trace!(
                    { progress_events::fields::TYPE } = progress_events::FILES,
                    { progress_events::fields::CURRENT } = files.len(),
                    "Evaluated {} files",
                    files.len()
                );
            });
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
                    warn!("Failed to parse Nix internal log: {} - line: {}", e, line);
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
            .and_then(|guard| guard.clone());

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
                        && let Some((_, activity_info)) = activities
                            .iter()
                            .find(|(_, info)| info.activity_type == ActivityType::Build)
                    {
                        activity_info.span.in_scope(|| {
                            info!(
                                { operation_fields::PHASE } = %phase,
                                { status_events::fields::STATUS } = status_events::ACTIVE,
                                "Build phase: {}", phase
                            );
                        });
                    }
                }
                InternalLog::Msg { level, ref msg, .. } => {
                    // First check if this is a file evaluation message
                    if let Some(op) = Op::from_internal_log(&log)
                        && let Op::EvaluatedFile { source } = op
                    {
                        self.handle_file_evaluation(operation_id.clone(), source);
                        return;
                    }

                    // Handle regular log messages from Nix builds
                    if level <= Verbosity::Warn {
                        match level {
                            Verbosity::Error => error!("{msg}"),
                            Verbosity::Warn => warn!("{msg}"),
                            _ => info!("{msg}"),
                        }
                    }
                }
            }
        }
    }

    /// Handle the start of a Nix activity
    fn handle_activity_start(
        &self,
        operation_id: OperationId,
        activity_id: u64,
        activity_type: ActivityType,
        text: String,
        fields: Vec<Field>,
    ) {
        match activity_type {
            ActivityType::Build => {
                let derivation_path = fields
                    .first()
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

                let message = if let Some(ref m) = machine {
                    format!("Building {} on {}", derivation_name, m)
                } else {
                    format!("Building {}", derivation_name)
                };

                let span = info_span!(
                    "nix_build",
                    { operation_fields::TYPE } = operation_types::BUILD,
                    { operation_fields::NAME } = %derivation_name,
                    { operation_fields::SHORT_NAME } = %derivation_name,
                    { operation_fields::DERIVATION } = %derivation_path,
                    { operation_fields::MACHINE } = ?machine,
                    { nix_fields::ACTIVITY_ID } = activity_id
                );
                span.in_scope(|| {
                    info!(
                        { status_events::fields::STATUS } = status_events::STARTING,
                        "{}", message
                    );
                });

                if let Ok(mut activities) = self.active_activities.lock() {
                    activities.insert(
                        activity_id,
                        NixActivityInfo {
                            operation_id,
                            activity_type,
                            span,
                        },
                    );
                }
            }
            ActivityType::QueryPathInfo => {
                if let (Some(Field::String(store_path)), Some(Field::String(substituter))) =
                    (fields.first(), fields.get(1))
                {
                    let package_name = extract_package_name(store_path);

                    let span = info_span!(
                        "nix_query",
                        { operation_fields::TYPE } = operation_types::QUERY,
                        { operation_fields::NAME } = %package_name,
                        { operation_fields::SHORT_NAME } = %package_name,
                        { operation_fields::STORE_PATH } = %store_path,
                        { operation_fields::SUBSTITUTER } = %substituter,
                        { nix_fields::ACTIVITY_ID } = activity_id
                    );
                    span.in_scope(|| {
                        info!(
                            { status_events::fields::STATUS } = status_events::STARTING,
                            "Querying {}", package_name
                        );
                    });

                    if let Ok(mut activities) = self.active_activities.lock() {
                        activities.insert(
                            activity_id,
                            NixActivityInfo {
                                operation_id,
                                activity_type,
                                span,
                            },
                        );
                    }
                }
            }
            ActivityType::CopyPath => {
                // CopyPath is the actual download activity that shows byte progress
                if let (Some(Field::String(store_path)), Some(Field::String(substituter))) =
                    (fields.first(), fields.get(1))
                {
                    let package_name = extract_package_name(store_path);

                    let span = info_span!(
                        "nix_download",
                        { operation_fields::TYPE } = operation_types::DOWNLOAD,
                        { operation_fields::NAME } = %package_name,
                        { operation_fields::SHORT_NAME } = %package_name,
                        { operation_fields::STORE_PATH } = %store_path,
                        { operation_fields::SUBSTITUTER } = %substituter,
                        { nix_fields::ACTIVITY_ID } = activity_id
                    );
                    span.in_scope(|| {
                        info!(
                            { status_events::fields::STATUS } = status_events::STARTING,
                            "Downloading {}", package_name
                        );
                    });

                    if let Ok(mut activities) = self.active_activities.lock() {
                        activities.insert(
                            activity_id,
                            NixActivityInfo {
                                operation_id,
                                activity_type,
                                span,
                            },
                        );
                    }
                }
            }
            ActivityType::FetchTree => {
                // FetchTree activities show when fetching Git repos, tarballs, etc.
                let span = info_span!(
                    "nix_fetch_tree",
                    { operation_fields::TYPE } = operation_types::FETCH_TREE,
                    { operation_fields::NAME } = %text,
                    { operation_fields::SHORT_NAME } = %text,
                    { nix_fields::ACTIVITY_ID } = activity_id
                );
                span.in_scope(|| {
                    info!(
                        { status_events::fields::STATUS } = status_events::STARTING,
                        "Fetching {}", text
                    );
                });

                if let Ok(mut activities) = self.active_activities.lock() {
                    activities.insert(
                        activity_id,
                        NixActivityInfo {
                            operation_id,
                            activity_type,
                            span,
                        },
                    );
                }
            }
            _ => {
                // For other activity types, we can add support as needed
                trace!("Unhandled Nix activity type: {:?}", activity_type);
            }
        }
    }

    /// Handle the stop of a Nix activity
    fn handle_activity_stop(&self, activity_id: u64, success: bool) {
        if let Ok(mut activities) = self.active_activities.lock()
            && let Some(activity_info) = activities.remove(&activity_id)
        {
            // If this is the last activity, flush any pending evaluation updates
            if activities.is_empty() {
                self.flush_evaluation_updates();
            }

            match activity_info.activity_type {
                ActivityType::Build => {
                    activity_info.span.in_scope(|| {
                        if success {
                            info!(
                                { status_events::fields::STATUS } = status_events::COMPLETED,
                                "Build completed"
                            );
                        } else {
                            warn!(
                                { status_events::fields::STATUS } = status_events::FAILED,
                                "Build failed"
                            );
                        }
                    });
                }
                ActivityType::CopyPath => {
                    activity_info.span.in_scope(|| {
                        if success {
                            info!(
                                { status_events::fields::STATUS } = status_events::COMPLETED,
                                "Download completed"
                            );
                        } else {
                            warn!(
                                { status_events::fields::STATUS } = status_events::FAILED,
                                "Download failed"
                            );
                        }
                    });
                }
                ActivityType::QueryPathInfo => {
                    activity_info.span.in_scope(|| {
                        if success {
                            debug!(
                                { status_events::fields::STATUS } = status_events::COMPLETED,
                                "Query completed"
                            );
                        } else {
                            warn!(
                                { status_events::fields::STATUS } = status_events::FAILED,
                                "Query failed"
                            );
                        }
                    });
                }
                ActivityType::FetchTree => {
                    activity_info.span.in_scope(|| {
                        if success {
                            info!(
                                { status_events::fields::STATUS } = status_events::COMPLETED,
                                "Fetch completed"
                            );
                        } else {
                            warn!(
                                { status_events::fields::STATUS } = status_events::FAILED,
                                "Fetch failed"
                            );
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
                        && let Some(activity_info) = activities.get(&activity_id)
                    {
                        activity_info.span.in_scope(|| {
                            debug!(
                                { progress_events::fields::TYPE } = progress_events::GENERIC,
                                { progress_events::fields::CURRENT } = done,
                                { progress_events::fields::TOTAL } = expected,
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
                            && let Some(activity_info) = activities.get(&activity_id)
                        {
                            // Only CopyPath activities have byte-based download progress
                            if activity_info.activity_type == ActivityType::CopyPath {
                                let percent = if let Some(total) = total_bytes {
                                    Some((*downloaded as f64 / total as f64) * 100.0)
                                } else {
                                    None
                                };

                                activity_info.span.in_scope(|| {
                                    if let Some(pct) = percent {
                                        debug!(
                                            { progress_events::fields::TYPE } = progress_events::BYTES,
                                            { progress_events::fields::CURRENT } = downloaded,
                                            { progress_events::fields::TOTAL } = ?total_bytes,
                                            { progress_events::fields::PERCENTAGE } = pct,
                                            "Download progress: {:.1}%", pct
                                        );
                                    } else {
                                        debug!(
                                            { progress_events::fields::TYPE } = progress_events::BYTES,
                                            { progress_events::fields::CURRENT } = downloaded,
                                            "Downloaded {} bytes", downloaded
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
                    && activity_info.activity_type == ActivityType::Build
                {
                    activity_info.span.in_scope(|| {
                        info!(
                            { operation_fields::PHASE } = %phase,
                            { status_events::fields::STATUS } = status_events::ACTIVE,
                            "Build phase: {}", phase
                        );
                    });
                }
            }
            ResultType::BuildLogLine => {
                // Handle build log output
                if let Some(Field::String(log_line)) = fields.first()
                    && let Ok(activities) = self.active_activities.lock()
                    && let Some(activity_info) = activities.get(&activity_id)
                {
                    activity_info.span.in_scope(|| {
                        info!(
                            line = %log_line,
                            "Build output: {}", log_line
                        );
                    });
                }
            }
            _ => {
                // Handle other result types as needed
                trace!("Unhandled Nix result type: {:?}", result_type);
            }
        }
    }

    /// Handle file evaluation events
    fn handle_file_evaluation(&self, _operation_id: OperationId, file_path: std::path::PathBuf) {
        const BATCH_SIZE: usize = 5; // Reduced from 10 for more responsive updates
        const BATCH_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(100); // Reduced from 200ms

        if let Ok(mut state) = self.evaluation_state.lock() {
            let file_path_str = file_path.display().to_string();

            // If this is the first file, create and store the evaluation span
            if state.total_files_evaluated == 0 && state.pending_files.is_empty() {
                let span = info_span!(
                    "nix_evaluation",
                    { operation_fields::TYPE } = operation_types::EVALUATE,
                    { operation_fields::NAME } = "Nix evaluation",
                    { operation_fields::SHORT_NAME } = "Eval"
                );
                span.in_scope(|| {
                    info!(
                        { status_events::fields::STATUS } = status_events::STARTING,
                        "Starting Nix evaluation: {}", file_path_str
                    );
                });
                state.span = Some(span);
            }

            // Add to pending files
            state.pending_files.push_back(file_path_str);
            state.total_files_evaluated += 1;

            // Check if we should send a batch update
            let now = Instant::now();
            let batch_full = state.pending_files.len() >= BATCH_SIZE;
            let timeout_exceeded = state
                .last_progress_update
                .is_some_and(|last| now.duration_since(last) >= BATCH_TIMEOUT);
            let should_send = batch_full || timeout_exceeded;

            if should_send && !state.pending_files.is_empty() {
                let files: Vec<String> = state.pending_files.drain(..).collect();

                // Emit progress event within the stored evaluation span
                if let Some(ref span) = state.span {
                    span.in_scope(|| {
                        info!(
                            { progress_events::fields::TYPE } = progress_events::FILES,
                            { progress_events::fields::CURRENT } = state.total_files_evaluated,
                            { status_events::fields::STATUS } = status_events::ACTIVE,
                            "Evaluated {} files (total: {})",
                            files.len(),
                            state.total_files_evaluated
                        );
                    });
                }
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
        && let Some(slash_pos) = path[..dash_pos].rfind('/')
    {
        return path[slash_pos + 1..].to_string();
    }

    // Fallback: just take the filename
    path.split('/').next_back().unwrap_or(path).to_string()
}

/// Extract a human-readable package name from a store path
fn extract_package_name(store_path: &str) -> String {
    // Extract the name part after the hash (format: /nix/store/hash-name)
    if let Some(dash_pos) = store_path.rfind('-')
        && let Some(slash_pos) = store_path[..dash_pos].rfind('/')
    {
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
