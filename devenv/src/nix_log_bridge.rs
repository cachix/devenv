use devenv_eval_cache::Op;
use devenv_eval_cache::internal_log::{ActivityType, Field, InternalLog, ResultType, Verbosity};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tracing::{Span, debug, debug_span, error, info, trace, warn};

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
        if let Ok(mut state) = self.evaluation_state.lock()
            && !state.pending_files.is_empty()
        {
            if let Ok(current) = self.current_operation_id.lock() {
                if current.is_some() {
                    let files: Vec<String> = state.pending_files.drain(..).collect();
                    trace!("Flushing {} pending evaluation files", files.len());

                    // Emit tracing event for evaluation progress using stored span
                    if let Some(ref span) = state.span {
                        span.in_scope(|| {
                            trace!(
                                devenv.user_message = format!("Evaluated {} files", files.len()),
                                files = ?files,
                                "Evaluated {} files", files.len()
                            );
                        });
                    }
                } else {
                    warn!(
                        "No operation ID available for flushing {} pending files",
                        state.pending_files.len()
                    );
                }
            } else {
                warn!("Failed to lock operation ID for flushing");
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
                                devenv.user_message = format!("Build phase: {}", phase),
                                phase = %phase,
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

                let span = debug_span!(
                    "nix_derivation",
                    devenv.user_message = %message,
                    activity_id = activity_id,
                    derivation_path = %derivation_path,
                    derivation_name = %derivation_name,
                    machine = ?machine
                );
                span.in_scope(|| {
                    info!("{}", message);
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

                    let span = debug_span!(
                        "nix_query",
                        devenv.user_message = format!("Querying {}", package_name),
                        activity_id = activity_id,
                        store_path = %store_path,
                        package_name = %package_name,
                        substituter = %substituter
                    );
                    span.in_scope(|| {
                        info!("Querying {}", package_name);
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

                    let span = debug_span!(
                        "nix_download",
                        devenv.user_message = format!("Downloading {}", package_name),
                        activity_id = activity_id,
                        store_path = %store_path,
                        package_name = %package_name,
                        substituter = %substituter
                    );
                    span.in_scope(|| {
                        info!("Downloading {}", package_name);
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
                let span = debug_span!(
                    "fetch_tree",
                    devenv.user_message = format!("Fetching {}", text),
                    activity_id = activity_id,
                    message = %text
                );
                span.in_scope(|| {
                    info!("Fetching {}", text);
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
                    let message = if success {
                        "Build completed"
                    } else {
                        "Build failed"
                    };
                    activity_info.span.in_scope(|| {
                        if success {
                            info!("{}", message);
                        } else {
                            warn!("{}", message);
                        }
                    });
                }
                ActivityType::CopyPath => {
                    let message = if success {
                        "Download completed"
                    } else {
                        "Download failed"
                    };
                    activity_info.span.in_scope(|| {
                        if success {
                            info!("{}", message);
                        } else {
                            warn!("{}", message);
                        }
                    });
                }
                ActivityType::QueryPathInfo => {
                    activity_info.span.in_scope(|| {
                        if success {
                            debug!("Query completed");
                        } else {
                            warn!("Query failed");
                        }
                    });
                }
                ActivityType::FetchTree => {
                    let message = if success {
                        "Fetch completed"
                    } else {
                        "Fetch failed"
                    };
                    activity_info.span.in_scope(|| {
                        if success {
                            info!("{}", message);
                        } else {
                            warn!("{}", message);
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
                                devenv.user_message =
                                    format!("Progress: {}/{} done", done, expected),
                                done = done,
                                expected = expected,
                                running = running,
                                failed = failed,
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
                                let message = if let Some(total) = total_bytes {
                                    let percent = (*downloaded as f64 / total as f64) * 100.0;
                                    format!("Download progress: {:.1}%", percent)
                                } else {
                                    format!("Downloaded {} bytes", downloaded)
                                };

                                activity_info.span.in_scope(|| {
                                    debug!(
                                        devenv.user_message = %message,
                                        bytes_downloaded = downloaded,
                                        total_bytes = ?total_bytes,
                                        "{}", message
                                    );
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
                            devenv.user_message = format!("Build phase: {}", phase),
                            phase = %phase,
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
                let span = debug_span!(
                    "nix_evaluation",
                    devenv.user_message = "Evaluating Nix files",
                    first_file = %file_path_str
                );
                span.in_scope(|| {
                    info!("Starting Nix evaluation: {}", file_path_str);
                });
                state.span = Some(span);
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

                // Emit progress event within the stored evaluation span
                if let Some(ref span) = state.span {
                    span.in_scope(|| {
                        info!(
                            devenv.user_message = format!("Evaluating Nix files ({} total)", state.total_files_evaluated),
                            files = ?files,
                            total_files_evaluated = state.total_files_evaluated,
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
