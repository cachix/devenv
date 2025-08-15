use crate::{
    ActivityProgress, LogMessage, NixActivityState, NixActivityType, Operation, OperationId,
    OperationState,
};
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
}

/// Unified activity structure
#[derive(Debug, Clone)]
pub struct Activity {
    pub id: u64,
    pub activity_type: NixActivityType,
    pub operation_id: OperationId,
    pub name: String,
    pub short_name: String,
    pub parent_operation: Option<OperationId>,
    pub start_time: Instant,
    pub state: NixActivityState,
    pub progress: Option<ActivityProgress>,

    /// Activity-specific data stored as key-value pairs
    pub data: HashMap<String, String>,
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
        }
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
            .or_insert_with(VecDeque::new);

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
                    && matches!(activity.activity_type, NixActivityType::Build)
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
                .filter(|a| &a.operation_id == &operation.id)
                .collect();

            if op_activities.is_empty() || operation.data.contains_key("evaluation_count") {
                activities.push(DisplayActivity {
                    activity: Activity {
                        id: 0, // Pseudo activity
                        activity_type: NixActivityType::Evaluating,
                        operation_id: operation.id.clone(),
                        name: operation.message.clone(),
                        short_name: operation.message.clone(),
                        parent_operation: operation.parent.clone(),
                        start_time: operation.start_time,
                        state: NixActivityState::Active,
                        progress: None,
                        data: operation.data.clone(),
                    },
                    depth,
                });
            }
        }

        // Add actual activities
        for activity in self.activities.values() {
            if &activity.operation_id == &operation.id {
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
            match (&activity.activity_type, &activity.state) {
                (NixActivityType::Build, NixActivityState::Active) => summary.active_builds += 1,
                (NixActivityType::Build, NixActivityState::Completed { success: true, .. }) => {
                    summary.completed_builds += 1;
                }
                (NixActivityType::Build, NixActivityState::Completed { success: false, .. }) => {
                    summary.failed_builds += 1;
                }
                (NixActivityType::Download, NixActivityState::Active) => {
                    summary.active_downloads += 1
                }
                (NixActivityType::Download, NixActivityState::Completed { success: true, .. }) => {
                    summary.completed_downloads += 1;
                }
                (NixActivityType::Query, NixActivityState::Active) => summary.active_queries += 1,
                (NixActivityType::Query, NixActivityState::Completed { success: true, .. }) => {
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

    /// Unified activity information for compatibility
    pub fn get_all_activities(&self) -> Vec<ActivityInfo> {
        self.get_display_activities()
            .into_iter()
            .map(|da| ActivityInfo::from_activity(da.activity, da.depth))
            .collect()
    }

    /// Get only active activities as ActivityInfo for display
    pub fn get_active_activity_infos(&self) -> Vec<ActivityInfo> {
        let mut activities: Vec<ActivityInfo> = self
            .get_display_activities()
            .into_iter()
            .filter(|da| matches!(da.activity.state, NixActivityState::Active))
            .map(|da| ActivityInfo::from_activity(da.activity, da.depth))
            .collect();

        // Sort by activity type priority: evaluating first, then builds, downloads, queries last
        activities.sort_by(|a, b| {
            activity_type_priority(&a.activity_type).cmp(&activity_type_priority(&b.activity_type))
        });

        activities
    }
}

/// Get priority order for activity types (lower numbers = higher priority)
fn activity_type_priority(activity_type: &crate::NixActivityType) -> u8 {
    use crate::NixActivityType;
    match activity_type {
        NixActivityType::Evaluating => 0,
        NixActivityType::Build => 1,
        NixActivityType::Download => 2,
        NixActivityType::Query => 3,
        _ => 4, // Other types last
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

/// Compatibility struct for existing code
#[derive(Debug, Clone)]
pub struct ActivityInfo {
    pub activity_type: NixActivityType,
    pub activity_id: Option<u64>,
    pub operation_id: OperationId,
    pub name: String,
    pub short_name: String,
    pub parent: Option<OperationId>,
    pub depth: usize,
    pub start_time: Instant,
    pub state: NixActivityState,
    pub current_phase: Option<String>,
    pub generic_progress: Option<ActivityProgress>,
    pub bytes_downloaded: Option<u64>,
    pub total_bytes: Option<u64>,
    pub download_speed: Option<f64>,
    pub substituter: Option<String>,
    pub operation_parent: Option<OperationId>,
    pub evaluation_count: Option<String>,
}

impl ActivityInfo {
    fn from_activity(activity: Activity, depth: usize) -> Self {
        let current_phase = activity.data.get("phase").cloned();
        let bytes_downloaded = activity
            .data
            .get("bytes_downloaded")
            .and_then(|s| s.parse::<u64>().ok());
        let total_bytes = activity
            .data
            .get("total_bytes")
            .and_then(|s| s.parse::<u64>().ok());
        let download_speed = activity
            .data
            .get("download_speed")
            .and_then(|s| s.parse::<f64>().ok());
        let substituter = activity.data.get("substituter").cloned();
        let evaluation_count = activity.data.get("evaluation_count").cloned();

        Self {
            activity_type: activity.activity_type,
            activity_id: if activity.id == 0 {
                None
            } else {
                Some(activity.id)
            },
            operation_id: activity.operation_id,
            name: activity.name,
            short_name: activity.short_name,
            parent: activity.parent_operation.clone(),
            depth,
            start_time: activity.start_time,
            state: activity.state,
            current_phase,
            generic_progress: activity.progress,
            bytes_downloaded,
            total_bytes,
            download_speed,
            substituter,
            operation_parent: activity.parent_operation,
            evaluation_count,
        }
    }
}

impl Default for Model {
    fn default() -> Self {
        Self::new()
    }
}
