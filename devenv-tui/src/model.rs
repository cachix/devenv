use crate::{
    ActivityProgress, FetchTreeInfo, LogMessage, NixActivityState, NixActivityType, NixBuildInfo,
    NixDerivationInfo, NixDownloadInfo, NixQueryInfo, Operation, OperationId, OperationState,
};
use std::collections::{HashMap, VecDeque};
use std::time::Instant;

/// Maximum number of log messages to keep in memory
const MAX_LOG_MESSAGES: usize = 1000;

/// Maximum number of log lines to keep per build
const MAX_LOG_LINES_PER_BUILD: usize = 1000;

/// The main application model following The Elm Architecture
/// This represents the complete state of the TUI application
#[derive(Debug)]
pub struct Model {
    /// All tracked operations
    pub operations: HashMap<OperationId, Operation>,

    /// Message log for general logging
    pub message_log: VecDeque<LogMessage>,

    /// Active Nix builds (legacy format)
    pub nix_builds: HashMap<OperationId, NixBuildInfo>,

    /// Active Nix derivations (internal-json format)
    pub nix_derivations: HashMap<u64, NixDerivationInfo>,

    /// Active Nix downloads
    pub nix_downloads: HashMap<u64, NixDownloadInfo>,

    /// Active Nix store queries
    pub nix_queries: HashMap<u64, NixQueryInfo>,

    /// Active fetch tree operations
    pub fetch_trees: HashMap<u64, FetchTreeInfo>,

    /// Root operations (operations without parents)
    pub root_operations: Vec<OperationId>,

    /// Build logs indexed by activity ID
    pub build_logs: HashMap<u64, VecDeque<String>>,

    /// Progress information for activities
    pub activity_progress: HashMap<u64, ActivityProgress>,

    /// UI state
    pub ui: UiState,

    /// Application state
    pub app_state: AppState,

    /// Completed messages to print above the TUI area
    pub completed_messages: Vec<String>,
}

/// UI-specific state
#[derive(Debug)]
pub struct UiState {
    /// Current spinner frame index
    pub spinner_frame: usize,

    /// Last time the spinner was updated
    pub last_spinner_update: Instant,

    /// Viewport height for the TUI
    pub viewport_height: u16,

    /// Minimum viewport height
    pub min_viewport_height: u16,

    /// Maximum viewport height
    pub max_viewport_height: u16,

    /// Selected operation ID (if any)
    pub selected_operation: Option<OperationId>,

    /// Selected activity index (for navigation)
    pub selected_activity_index: Option<usize>,

    /// Scroll offset for logs
    pub log_scroll_offset: usize,

    /// Scroll offset for activities
    pub activity_scroll_offset: usize,

    /// Scroll position for activities
    pub activity_scroll_position: usize,

    /// Whether to show detailed view
    pub show_details: bool,

    /// Whether to show expanded logs (all logs vs just 10 lines)
    pub show_expanded_logs: bool,

    /// Height available for activities display
    pub activities_visible_height: u16,
}

/// Application-level state
#[derive(Debug, PartialEq)]
pub enum AppState {
    /// Normal running state
    Running,

    /// Application is shutting down
    ShuttingDown,

    /// Application has been shut down
    Shutdown,
}

/// Unified activity information for display
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

    // Download-specific fields
    pub bytes_downloaded: Option<u64>,
    pub total_bytes: Option<u64>,
    pub download_speed: Option<f64>,

    // Query-specific fields
    pub substituter: Option<String>,

    // Operation-specific fields
    pub operation_parent: Option<OperationId>,
    pub evaluation_count: Option<String>,
}

impl Model {
    /// Create a new Model with default values
    pub fn new() -> Self {
        Self {
            operations: HashMap::new(),
            message_log: VecDeque::new(),
            nix_builds: HashMap::new(),
            nix_derivations: HashMap::new(),
            nix_downloads: HashMap::new(),
            nix_queries: HashMap::new(),
            fetch_trees: HashMap::new(),
            root_operations: Vec::new(),
            build_logs: HashMap::new(),
            activity_progress: HashMap::new(),
            ui: UiState {
                spinner_frame: 0,
                last_spinner_update: Instant::now(),
                viewport_height: 10,
                min_viewport_height: 10,
                max_viewport_height: 40,
                selected_operation: None,
                selected_activity_index: None,
                log_scroll_offset: 0,
                activity_scroll_offset: 0,
                activity_scroll_position: 0,
                show_details: false,
                show_expanded_logs: false,
                activities_visible_height: 5,
            },
            app_state: AppState::Running,
            completed_messages: Vec::new(),
        }
    }

    /// Add a log message to the message log, maintaining size limit
    pub fn add_log_message(&mut self, message: LogMessage) {
        self.message_log.push_back(message);

        // Keep log size bounded
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

        // Add new line, removing oldest if at capacity
        if logs.len() >= MAX_LOG_LINES_PER_BUILD {
            logs.pop_front();
        }
        logs.push_back(line);
    }

    /// Clean up completed operations that are older than a certain threshold
    pub fn cleanup_old_operations(&mut self, max_age: std::time::Duration) {
        let now = Instant::now();

        let mut to_remove = Vec::new();
        for (id, operation) in &self.operations {
            if let crate::OperationState::Complete { .. } = operation.state {
                if now.duration_since(operation.start_time) > max_age {
                    to_remove.push(id.clone());
                }
            }
        }

        for id in to_remove {
            self.operations.remove(&id);
            self.root_operations.retain(|op_id| *op_id != id);
        }
    }

    /// Get all activities as a flat list for display
    pub fn get_all_activities(&self) -> Vec<ActivityInfo> {
        let mut activities = Vec::new();
        let mut processed_operations = std::collections::HashSet::new();

        // Start with root operations
        for root_id in &self.root_operations {
            if let Some(operation) = self.operations.get(root_id) {
                self.add_operation_activities(
                    &mut activities,
                    operation,
                    0,
                    &mut processed_operations,
                );
            }
        }

        activities
    }

    /// Get only active activities
    pub fn get_active_activities(&self) -> Vec<ActivityInfo> {
        self.get_all_activities()
            .into_iter()
            .filter(|activity| matches!(activity.state, NixActivityState::Active))
            .collect()
    }

    /// Recursively add activities for an operation and its children
    fn add_operation_activities(
        &self,
        activities: &mut Vec<ActivityInfo>,
        operation: &Operation,
        depth: usize,
        processed_operations: &mut std::collections::HashSet<OperationId>,
    ) {
        // Skip if we've already processed this operation
        if !processed_operations.insert(operation.id.clone()) {
            return;
        }
        // Only show evaluation activity for operations that are actually evaluating
        if matches!(operation.state, OperationState::Active) {
            // Check if this operation has evaluation data
            let has_evaluation_data = operation.data.contains_key("evaluation_count")
                || operation.data.contains_key("evaluation_file");

            // Only create an evaluating activity if:
            // 1. The operation has evaluation data, OR
            // 2. The operation has no other specific activities (derivations, downloads, etc.)
            let has_other_activities = self
                .nix_derivations
                .values()
                .any(|d| &d.operation_id == &operation.id)
                || self
                    .nix_downloads
                    .values()
                    .any(|d| &d.operation_id == &operation.id)
                || self
                    .nix_queries
                    .values()
                    .any(|q| &q.operation_id == &operation.id)
                || self
                    .fetch_trees
                    .values()
                    .any(|f| &f.operation_id == &operation.id);

            if has_evaluation_data || !has_other_activities {
                activities.push(ActivityInfo {
                    activity_type: NixActivityType::Evaluating,
                    activity_id: None,
                    operation_id: operation.id.clone(),
                    name: operation.message.clone(),
                    short_name: operation.message.clone(),
                    parent: operation.parent.clone(),
                    depth,
                    start_time: operation.start_time,
                    state: NixActivityState::Active,
                    current_phase: None,
                    generic_progress: None,
                    bytes_downloaded: None,
                    total_bytes: None,
                    download_speed: None,
                    substituter: None,
                    operation_parent: operation.parent.clone(),
                    evaluation_count: operation.data.get("evaluation_count").cloned(),
                });
            }
        }

        // Get all Nix activities for this operation
        let fetch_trees: Vec<_> = self
            .fetch_trees
            .values()
            .filter(|ft| &ft.operation_id == &operation.id)
            .cloned()
            .collect();

        // Add active fetch tree activities
        for fetch_tree in fetch_trees {
            let activity = ActivityInfo::from_fetch_tree(
                fetch_tree.clone(),
                operation.parent.clone(),
                depth + 1,
            );
            activities.push(activity);
        }

        // Add build activities
        for derivation in self.nix_derivations.values() {
            if &derivation.operation_id == &operation.id {
                let mut activity = ActivityInfo::from_derivation(
                    derivation.clone(),
                    operation.parent.clone(),
                    depth + 1,
                );
                // Get generic progress if available
                activity.generic_progress =
                    self.activity_progress.get(&derivation.activity_id).cloned();
                activities.push(activity);
            }
        }

        // Add download activities
        for download in self.nix_downloads.values() {
            if &download.operation_id == &operation.id {
                let mut activity = ActivityInfo::from_download(
                    download.clone(),
                    operation.parent.clone(),
                    depth + 1,
                );
                // Get generic progress if available
                activity.generic_progress =
                    self.activity_progress.get(&download.activity_id).cloned();
                activities.push(activity);
            }
        }

        // Add query activities
        for query in self.nix_queries.values() {
            if &query.operation_id == &operation.id {
                let mut activity =
                    ActivityInfo::from_query(query.clone(), operation.parent.clone(), depth + 1);
                // Get generic progress if available
                activity.generic_progress = self.activity_progress.get(&query.activity_id).cloned();
                activities.push(activity);
            }
        }

        // Recursively add children
        for child_id in &operation.children {
            if let Some(child) = self.operations.get(child_id) {
                self.add_operation_activities(activities, child, depth + 1, processed_operations);
            }
        }
    }

    /// Calculate summary statistics
    pub fn calculate_summary(&self) -> ActivitySummary {
        let mut summary = ActivitySummary::default();

        // Get ALL operations (not just active ones) to include completed activities
        for op in self.operations.values() {
            // Count builds by state
            for derivation in self.nix_derivations.values() {
                if &derivation.operation_id == &op.id {
                    match derivation.state {
                        NixActivityState::Active => summary.active_builds += 1,
                        NixActivityState::Completed { success, .. } => {
                            if success {
                                summary.completed_builds += 1;
                            } else {
                                summary.failed_builds += 1;
                            }
                        }
                    }
                }
            }

            // Count downloads by state
            for download in self.nix_downloads.values() {
                if &download.operation_id == &op.id {
                    match download.state {
                        NixActivityState::Active => summary.active_downloads += 1,
                        NixActivityState::Completed { success: true, .. } => {
                            summary.completed_downloads += 1;
                        }
                        _ => {}
                    }
                }
            }

            // Count queries by state
            for query in self.nix_queries.values() {
                if &query.operation_id == &op.id {
                    match query.state {
                        NixActivityState::Active => summary.active_queries += 1,
                        NixActivityState::Completed { success: true, .. } => {
                            summary.completed_queries += 1;
                        }
                        _ => {}
                    }
                }
            }
        }

        summary.total_builds =
            summary.active_builds + summary.completed_builds + summary.failed_builds;
        summary
    }

    /// Calculate total duration from the earliest operation start to now
    pub fn get_total_duration(&self) -> Option<std::time::Duration> {
        // Find the earliest start time among all operations
        let earliest_start = self.operations.values().map(|op| op.start_time).min()?;

        Some(Instant::now().duration_since(earliest_start))
    }

    /// Get the currently selected activity
    pub fn get_selected_activity(&self) -> Option<ActivityInfo> {
        let index = self.ui.selected_activity_index?;
        let activities = self.get_active_activities();
        activities.get(index).cloned()
    }

    /// Get build logs for an activity
    pub fn get_build_logs(&self, activity_id: u64) -> Option<&VecDeque<String>> {
        self.build_logs.get(&activity_id)
    }
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

impl ActivityInfo {
    /// Create from fetch tree
    pub fn from_fetch_tree(
        fetch_tree: FetchTreeInfo,
        parent: Option<OperationId>,
        depth: usize,
    ) -> Self {
        Self {
            activity_type: NixActivityType::FetchTree,
            activity_id: Some(fetch_tree.activity_id),
            operation_id: fetch_tree.operation_id.clone(),
            name: fetch_tree.message.clone(),
            short_name: fetch_tree.message.clone(),
            parent: parent.clone(),
            depth,
            start_time: fetch_tree.start_time,
            state: fetch_tree.state,
            current_phase: None,
            generic_progress: None,
            bytes_downloaded: None,
            total_bytes: None,
            download_speed: None,
            substituter: None,
            operation_parent: parent,
            evaluation_count: None,
        }
    }

    /// Create from derivation
    pub fn from_derivation(
        derivation: NixDerivationInfo,
        parent: Option<OperationId>,
        depth: usize,
    ) -> Self {
        Self {
            activity_type: NixActivityType::Build,
            activity_id: Some(derivation.activity_id),
            operation_id: derivation.operation_id.clone(),
            name: derivation.derivation_path.clone(),
            short_name: derivation.derivation_name.clone(),
            parent: parent.clone(),
            depth,
            start_time: derivation.start_time,
            state: derivation.state,
            current_phase: derivation.current_phase,
            generic_progress: None,
            bytes_downloaded: None,
            total_bytes: None,
            download_speed: None,
            substituter: None,
            operation_parent: parent,
            evaluation_count: None,
        }
    }

    /// Create from download
    pub fn from_download(
        download: NixDownloadInfo,
        parent: Option<OperationId>,
        depth: usize,
    ) -> Self {
        Self {
            activity_type: NixActivityType::Download,
            activity_id: Some(download.activity_id),
            operation_id: download.operation_id.clone(),
            name: download.store_path.clone(),
            short_name: download.package_name.clone(),
            parent: parent.clone(),
            depth,
            start_time: download.start_time,
            state: download.state,
            current_phase: None,
            generic_progress: None,
            bytes_downloaded: Some(download.bytes_downloaded),
            total_bytes: download.total_bytes,
            download_speed: Some(download.download_speed),
            substituter: Some(download.substituter),
            operation_parent: parent,
            evaluation_count: None,
        }
    }

    /// Create from query
    pub fn from_query(query: NixQueryInfo, parent: Option<OperationId>, depth: usize) -> Self {
        Self {
            activity_type: NixActivityType::Query,
            activity_id: Some(query.activity_id),
            operation_id: query.operation_id.clone(),
            name: query.store_path.clone(),
            short_name: query.package_name.clone(),
            parent: parent.clone(),
            depth,
            start_time: query.start_time,
            state: query.state,
            current_phase: None,
            generic_progress: None,
            bytes_downloaded: None,
            total_bytes: None,
            download_speed: None,
            substituter: Some(query.substituter),
            operation_parent: parent,
            evaluation_count: None,
        }
    }
}

impl Default for Model {
    fn default() -> Self {
        Self::new()
    }
}
