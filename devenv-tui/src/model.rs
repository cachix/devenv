use crate::{LogMessage, NixActivityState, Operation, OperationId, OperationState};
use std::collections::{HashMap, VecDeque};
use std::time::Instant;

/// Maximum number of log messages to keep in memory
const MAX_LOG_MESSAGES: usize = 1000;

/// Maximum number of log lines to keep per build
const MAX_LOG_LINES_PER_BUILD: usize = 1000;

/// The main application model
#[derive(Debug)]
pub struct Model {
    /// All tracked operations
    pub operations: HashMap<OperationId, Operation>,

    /// Message log for general logging
    pub message_log: VecDeque<LogMessage>,

    /// All Nix activities (unified)
    pub activities: HashMap<u64, Activity>,

    /// Root operations (operations without parents)
    pub root_operations: Vec<OperationId>,

    /// Build logs indexed by activity ID
    pub build_logs: HashMap<u64, VecDeque<String>>,

    /// UI state
    pub ui: UiState,

    /// Application state
    pub app_state: AppState,

    /// Completed messages to print above the TUI area
    pub completed_messages: Vec<String>,

    /// Evaluation file counters per activity
    evaluation_files_count: HashMap<u64, usize>,
}

/// Build-specific activity data
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BuildActivity {
    pub phase: Option<String>,
    pub log_stdout_lines: Vec<String>,
    pub log_stderr_lines: Vec<String>,
}

/// Download-specific activity data
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DownloadActivity {
    pub size_current: Option<u64>,
    pub size_total: Option<u64>,
    pub speed: Option<u64>,
    pub substituter: Option<String>,
}

/// Progress-specific activity data
#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct ProgressActivity {
    pub current: Option<u64>,
    pub total: Option<u64>,
    pub unit: Option<String>,
    pub percent: Option<f32>,
}

/// Query-specific activity data
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct QueryActivity {
    pub substituter: Option<String>,
}

/// Task-specific activity data
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TaskActivity {
    pub status: TaskDisplayStatus,
    pub duration: Option<std::time::Duration>,
}

/// Activity type variants with specific data
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ActivityVariant {
    Task(TaskActivity),
    UserOperation,
    Evaluating,
    Build(BuildActivity),
    Download(DownloadActivity),
    Query(QueryActivity),
    FetchTree,
    Unknown,
}

/// Unified activity structure with variant-specific data
#[derive(Debug, Clone)]
pub struct Activity {
    pub id: u64,
    pub operation_id: OperationId,
    pub name: String,
    pub short_name: String,
    pub parent_operation: Option<OperationId>,
    pub start_time: Instant,
    pub state: NixActivityState,
    pub detail: Option<String>,

    /// Activity variant with type-specific data
    pub variant: ActivityVariant,

    /// Progress tracking (can be used by any activity type)
    pub progress: Option<ProgressActivity>,
}

/// UI-specific state
#[derive(Debug)]
pub struct UiState {
    /// Current spinner frame index
    pub spinner_frame: usize,

    /// Last time the spinner was updated
    pub last_spinner_update: Instant,

    /// Viewport height configuration
    pub viewport: ViewportConfig,

    /// Selected activity ID (if any)
    pub selected_activity: Option<u64>,

    /// Scroll state
    pub scroll: ScrollState,

    /// View options
    pub view_options: ViewOptions,
}

#[derive(Debug)]
pub struct ViewportConfig {
    pub current: u16,
    pub min: u16,
    pub max: u16,
    pub activities_visible: u16,
}

#[derive(Debug)]
pub struct ScrollState {
    pub log_offset: usize,
    pub activity_position: usize,
}

#[derive(Debug)]
pub struct ViewOptions {
    pub show_details: bool,
    pub show_expanded_logs: bool,
}

/// Application-level state
#[derive(Debug, PartialEq)]
pub enum AppState {
    Running,
    ShuttingDown,
    Shutdown,
}

/// Display status for tasks
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum TaskDisplayStatus {
    Pending,
    Running,
    Success,
    Failed,
    Skipped,
    Cancelled,
}

impl Model {
    /// Create a new Model with default values
    pub fn new() -> Self {
        Self {
            operations: HashMap::new(),
            message_log: VecDeque::new(),
            activities: HashMap::new(),
            root_operations: Vec::new(),
            build_logs: HashMap::new(),
            ui: UiState {
                spinner_frame: 0,
                last_spinner_update: Instant::now(),
                viewport: ViewportConfig {
                    current: 10,
                    min: 10,
                    max: 40,
                    activities_visible: 5,
                },
                selected_activity: None,
                scroll: ScrollState {
                    log_offset: 0,
                    activity_position: 0,
                },
                view_options: ViewOptions {
                    show_details: false,
                    show_expanded_logs: false,
                },
            },
            app_state: AppState::Running,
            completed_messages: Vec::new(),
            evaluation_files_count: HashMap::new(),
        }
    }

    /// Add an activity and handle all internal state management and complex computations
    pub fn add_activity(&mut self, mut activity: Activity) {
        // Set parent operation if needed (complex lookup)
        if activity.parent_operation.is_none() {
            activity.parent_operation = self
                .operations
                .get(&activity.operation_id)
                .and_then(|op| op.parent.clone());
        }

        // Handle variant-specific initialization and tracking
        match &mut activity.variant {
            ActivityVariant::Download(download) => {
                // Initialize download tracking - remove speed calculations
                download.speed = None;

                // If we have progress data, update it
                if let (Some(current), Some(total)) = (download.size_current, download.size_total) {
                    // Any download-specific calculations would go here
                    let percent = if total > 0 {
                        Some((current as f32 / total as f32) * 100.0)
                    } else {
                        None
                    };

                    if let Some(p) = percent {
                        activity.progress = Some(ProgressActivity {
                            current: Some(current),
                            total: Some(total),
                            unit: Some("bytes".to_string()),
                            percent: Some(p),
                        });
                    }
                }
            }
            ActivityVariant::Evaluating => {
                // Initialize evaluation file counting and set initial detail
                self.evaluation_files_count.insert(activity.id, 0);
                if activity.detail.is_none() {
                    activity.detail = Some("0 files".to_string());
                }
            }
            ActivityVariant::Build(build_activity) => {
                // If we have build logs, could process them here
                // For now, just ensure consistent initialization
                if build_activity.phase.is_none() {
                    build_activity.phase = Some("preparing".to_string());
                }
            }
            ActivityVariant::Task(task_activity) => {
                // Task-specific processing could go here
                if task_activity.status == TaskDisplayStatus::Running {
                    // Could start timing or other tracking
                }
            }
            _ => {}
        }

        self.activities.insert(activity.id, activity);
    }

    /// Handle task start event - this should be called from tracing layer with proper IDs
    pub fn handle_task_start(
        &mut self,
        task_name: String,
        start_time: Instant,
        operation_id: OperationId,
        activity_id: u64,
    ) {
        let activity = Activity {
            id: activity_id,
            operation_id,
            name: task_name,
            short_name: "Task".to_string(),
            parent_operation: None, // Let add_activity handle this
            start_time,
            state: NixActivityState::Active,
            detail: None,
            variant: ActivityVariant::Task(TaskActivity {
                status: TaskDisplayStatus::Running,
                duration: None,
            }),
            progress: None,
        };

        self.add_activity(activity);
    }

    /// Add a log message to the message log, maintaining size limit
    pub fn add_log_message(&mut self, message: LogMessage) {
        self.message_log.push_back(message);

        if self.message_log.len() > MAX_LOG_MESSAGES {
            self.message_log.pop_front();
        }
    }

    /// Add a build log line for an activity
    pub fn add_build_log(&mut self, activity_id: u64, line: String) {
        let logs = self
            .build_logs
            .entry(activity_id)
            .or_default();

        if logs.len() >= MAX_LOG_LINES_PER_BUILD {
            logs.pop_front();
        }
        logs.push_back(line);
    }

    /// Get all active activities
    pub fn get_active_activities(&self) -> Vec<&Activity> {
        self.activities
            .values()
            .filter(|activity| matches!(activity.state, NixActivityState::Active))
            .collect()
    }

    /// Get all active build activities (for navigation)
    pub fn get_active_build_ids(&self) -> Vec<u64> {
        self.activities
            .iter()
            .filter(|(_, activity)| {
                matches!(activity.state, NixActivityState::Active)
                    && matches!(activity.variant, ActivityVariant::Build(_))
            })
            .map(|(id, _)| *id)
            .collect()
    }

    /// Select the next active build
    pub fn select_next_build(&mut self) {
        let active_builds = self.get_active_build_ids();

        if !active_builds.is_empty() {
            match self.ui.selected_activity {
                None => {
                    // Select first active build
                    self.ui.selected_activity = active_builds.first().copied();
                }
                Some(current_id) => {
                    // Find current position and select next
                    if let Some(current_pos) = active_builds.iter().position(|&id| id == current_id)
                    {
                        let next_pos = (current_pos + 1) % active_builds.len();
                        self.ui.selected_activity = Some(active_builds[next_pos]);
                    } else {
                        // Current selection is not an active build anymore, select first
                        self.ui.selected_activity = active_builds.first().copied();
                    }
                }
            }
        }
    }

    /// Select the previous active build
    pub fn select_previous_build(&mut self) {
        let active_builds = self.get_active_build_ids();

        if !active_builds.is_empty() {
            match self.ui.selected_activity {
                None => {
                    // Select last active build
                    self.ui.selected_activity = active_builds.last().copied();
                }
                Some(current_id) => {
                    // Find current position and select previous
                    if let Some(current_pos) = active_builds.iter().position(|&id| id == current_id)
                    {
                        let prev_pos = if current_pos == 0 {
                            active_builds.len() - 1
                        } else {
                            current_pos - 1
                        };
                        self.ui.selected_activity = Some(active_builds[prev_pos]);
                    } else {
                        // Current selection is not an active build anymore, select last
                        self.ui.selected_activity = active_builds.last().copied();
                    }
                }
            }
        }
    }

    /// Get activities for display (with depth calculation)
    pub fn get_display_activities(&self) -> Vec<DisplayActivity> {
        let mut activities = Vec::new();
        let mut processed = std::collections::HashSet::new();

        for root_id in &self.root_operations {
            if let Some(operation) = self.operations.get(root_id) {
                self.add_display_activities(&mut activities, operation, 0, &mut processed);
            }
        }

        activities
    }

    fn add_display_activities(
        &self,
        activities: &mut Vec<DisplayActivity>,
        operation: &Operation,
        depth: usize,
        processed: &mut std::collections::HashSet<OperationId>,
    ) {
        if !processed.insert(operation.id.clone()) {
            return;
        }

        // Add evaluation activity if operation is active
        if matches!(operation.state, OperationState::Active) {
            let op_activities: Vec<_> = self
                .activities
                .values()
                .filter(|a| a.operation_id == operation.id)
                .collect();

            // Create pseudo evaluation activity if this operation has evaluation data
            if operation.data.contains_key("evaluation_count") {
                activities.push(DisplayActivity {
                    activity: Activity {
                        id: 0, // Pseudo activity
                        operation_id: operation.id.clone(),
                        name: operation.message.clone(),
                        short_name: operation.message.clone(),
                        parent_operation: operation.parent.clone(),
                        start_time: operation.start_time,
                        state: NixActivityState::Active,
                        detail: operation
                            .data
                            .get("evaluation_count")
                            .map(|c| format!("{} files", c)),
                        variant: ActivityVariant::Evaluating,
                        progress: None,
                    },
                    depth,
                });
            } else if op_activities.is_empty() {
                // For user operations with no activities, display as UserOperation
                activities.push(DisplayActivity {
                    activity: Activity {
                        id: 0, // Pseudo activity
                        operation_id: operation.id.clone(),
                        name: operation.message.clone(),
                        short_name: operation.message.clone(),
                        parent_operation: operation.parent.clone(),
                        start_time: operation.start_time,
                        state: NixActivityState::Active,
                        detail: None,
                        variant: ActivityVariant::UserOperation,
                        progress: None,
                    },
                    depth,
                });
            }
        }

        // Add actual activities
        for activity in self.activities.values() {
            if activity.operation_id == operation.id {
                activities.push(DisplayActivity {
                    activity: activity.clone(),
                    depth: depth + 1,
                });
            }
        }

        // Add children
        for child_id in &operation.children {
            if let Some(child) = self.operations.get(child_id) {
                self.add_display_activities(activities, child, depth + 1, processed);
            }
        }
    }

    /// Calculate summary statistics
    pub fn calculate_summary(&self) -> ActivitySummary {
        let mut summary = ActivitySummary::default();

        for activity in self.activities.values() {
            match (&activity.variant, &activity.state) {
                (ActivityVariant::Build(_), NixActivityState::Active) => summary.active_builds += 1,
                (ActivityVariant::Build(_), NixActivityState::Completed { success: true, .. }) => {
                    summary.completed_builds += 1;
                }
                (ActivityVariant::Build(_), NixActivityState::Completed { success: false, .. }) => {
                    summary.failed_builds += 1;
                }
                (ActivityVariant::Download(_), NixActivityState::Active) => {
                    summary.active_downloads += 1
                }
                (
                    ActivityVariant::Download(_),
                    NixActivityState::Completed { success: true, .. },
                ) => {
                    summary.completed_downloads += 1;
                }
                (ActivityVariant::Query(_), NixActivityState::Active) => {
                    summary.active_queries += 1
                }
                (ActivityVariant::Query(_), NixActivityState::Completed { success: true, .. }) => {
                    summary.completed_queries += 1;
                }
                _ => {}
            }
        }

        summary.total_builds =
            summary.active_builds + summary.completed_builds + summary.failed_builds;
        summary
    }

    /// Get the currently selected activity
    pub fn get_selected_activity(&self) -> Option<&Activity> {
        self.ui
            .selected_activity
            .and_then(|id| self.activities.get(&id))
    }

    /// Get build logs for an activity
    pub fn get_build_logs(&self, activity_id: u64) -> Option<&VecDeque<String>> {
        self.build_logs.get(&activity_id)
    }

    /// Get total duration from the earliest operation start to now
    pub fn get_total_duration(&self) -> Option<std::time::Duration> {
        let earliest_start = self.operations.values().map(|op| op.start_time).min()?;
        Some(Instant::now().duration_since(earliest_start))
    }

    /// Get only active activities for display
    pub fn get_active_display_activities(&self) -> Vec<DisplayActivity> {
        let mut activities: Vec<DisplayActivity> = self
            .get_display_activities()
            .into_iter()
            .filter(|da| matches!(da.activity.state, NixActivityState::Active))
            .collect();

        // Sort by activity variant priority using built-in ordering
        activities.sort_by(|a, b| a.activity.variant.cmp(&b.activity.variant));

        activities
    }

    /// Handle task end event - update task status and duration
    pub fn handle_task_end(
        &mut self,
        task_name: String,
        duration: Option<std::time::Duration>,
        success: bool,
        error: Option<String>,
    ) {
        // Find and update the task activity
        for activity in self.activities.values_mut() {
            if activity.name == task_name {
                if let ActivityVariant::Task(ref mut task_activity) = activity.variant {
                    task_activity.status = if success {
                        TaskDisplayStatus::Success
                    } else {
                        TaskDisplayStatus::Failed
                    };
                    task_activity.duration = duration;
                }
                activity.state = if success {
                    NixActivityState::Completed {
                        success: true,
                        duration: duration.unwrap_or_default(),
                    }
                } else {
                    NixActivityState::Completed {
                        success: false,
                        duration: duration.unwrap_or_default(),
                    }
                };
                if let Some(err) = error {
                    activity.detail = Some(format!("Error: {}", err));
                }
                break;
            }
        }
    }
}

/// Activity with display depth
#[derive(Debug)]
pub struct DisplayActivity {
    pub activity: Activity,
    pub depth: usize,
}

/// Summary statistics for activities
#[derive(Debug, Default, Clone)]
pub struct ActivitySummary {
    pub active_builds: usize,
    pub completed_builds: usize,
    pub failed_builds: usize,
    pub total_builds: usize,
    pub active_downloads: usize,
    pub completed_downloads: usize,
    pub active_queries: usize,
    pub completed_queries: usize,
}

impl Default for Model {
    fn default() -> Self {
        Self::new()
    }
}
