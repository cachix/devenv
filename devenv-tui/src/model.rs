use crate::app::TuiConfig;
use devenv_activity::{
    ActivityEvent, ActivityLevel, ActivityOutcome, Build, Command, Evaluate, Fetch, FetchKind,
    Message, Operation, Task,
};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Configuration for limiting displayed child activities
#[derive(Debug, Clone)]
pub struct ChildActivityLimit {
    /// Maximum number of child lines to show
    pub max_lines: usize,
    /// How long completed items stay visible after completion
    pub linger_duration: Duration,
}

impl Default for ChildActivityLimit {
    fn default() -> Self {
        Self {
            max_lines: 5,
            linger_duration: Duration::from_secs(1),
        }
    }
}

/// Activity data model - contains only activity state from the event processor.
/// This is the only data that needs to be behind an RwLock.
#[derive(Debug)]
pub struct ActivityModel {
    pub message_log: VecDeque<Message>,
    pub activities: HashMap<u64, Activity>,
    pub root_activities: Vec<u64>,
    pub build_logs: HashMap<u64, Arc<VecDeque<String>>>,
    pub app_state: AppState,
    pub completed_messages: Vec<String>,
    next_message_id: u64,
    config: Arc<TuiConfig>,
    /// Set to true when Done event is received, signaling graceful shutdown
    done: bool,
}

impl Default for ActivityModel {
    fn default() -> Self {
        Self::with_config(Arc::new(TuiConfig::default()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BuildActivity {
    pub phase: Option<String>,
    pub log_stdout_lines: Vec<String>,
    pub log_stderr_lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DownloadActivity {
    pub size_current: Option<u64>,
    pub size_total: Option<u64>,
    pub speed: Option<u64>,
    pub substituter: Option<String>,
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct ProgressActivity {
    pub current: Option<u64>,
    pub total: Option<u64>,
    pub unit: Option<String>,
    pub percent: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct QueryActivity {
    pub substituter: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TaskActivity {
    pub status: TaskDisplayStatus,
    pub duration: Option<std::time::Duration>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct EvaluatingActivity {
    pub files_evaluated: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct MessageActivity {
    pub level: ActivityLevel,
    pub details: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ActivityVariant {
    Task(TaskActivity),
    UserOperation,
    Evaluating(EvaluatingActivity),
    Build(BuildActivity),
    Download(DownloadActivity),
    Query(QueryActivity),
    FetchTree,
    /// Devenv-specific operations (e.g., "Building shell", "Entering shell")
    Devenv,
    /// Standalone messages displayed as children of their parent activity
    Message(MessageActivity),
    Unknown,
}

/// Key-value detail/metadata for an activity
#[derive(Debug, Clone)]
pub struct ActivityDetail {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct Activity {
    pub id: u64,
    pub name: String,
    pub short_name: String,
    pub parent_id: Option<u64>,
    pub start_time: Instant,
    pub state: NixActivityState,
    /// When the activity completed (for lingering display)
    pub completed_at: Option<Instant>,
    pub detail: Option<String>,
    pub variant: ActivityVariant,
    pub progress: Option<ProgressActivity>,
    /// Additional details/metadata (shown when expanded)
    pub details: Vec<ActivityDetail>,
    /// Activity level for filtering (defaults to Info)
    pub level: ActivityLevel,
}

/// UI state - lives outside the RwLock, managed by the UI thread.
#[derive(Debug)]
pub struct UiState {
    pub viewport: ViewportConfig,
    pub selected_activity: Option<u64>,
    pub scroll: ScrollState,
    pub view_options: ViewOptions,
    pub terminal_size: TerminalSize,
    pub view_mode: ViewMode,
}

impl UiState {
    /// Create a new UiState, querying the terminal for its size.
    pub fn new() -> Self {
        let (width, height) = crossterm::terminal::size().unwrap_or((80, 24));
        Self {
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
            },
            terminal_size: TerminalSize { width, height },
            view_mode: ViewMode::Main,
        }
    }

    /// Update terminal size (call on resize events).
    pub fn set_terminal_size(&mut self, width: u16, height: u16) {
        self.terminal_size = TerminalSize { width, height };
    }

    /// Select the next activity from the list of selectable IDs.
    pub fn select_next_activity(&mut self, selectable: &[u64]) {
        if selectable.is_empty() {
            return;
        }
        match self.selected_activity {
            None => {
                self.selected_activity = selectable.first().copied();
            }
            Some(current_id) => {
                if let Some(current_pos) = selectable.iter().position(|&id| id == current_id) {
                    if current_pos + 1 < selectable.len() {
                        self.selected_activity = Some(selectable[current_pos + 1]);
                    }
                } else {
                    self.selected_activity = selectable.first().copied();
                }
            }
        }
    }

    /// Select the previous activity from the list of selectable IDs.
    pub fn select_previous_activity(&mut self, selectable: &[u64]) {
        if selectable.is_empty() {
            return;
        }
        match self.selected_activity {
            None => {
                self.selected_activity = selectable.first().copied();
            }
            Some(current_id) => {
                if let Some(current_pos) = selectable.iter().position(|&id| id == current_id) {
                    if current_pos > 0 {
                        self.selected_activity = Some(selectable[current_pos - 1]);
                    }
                } else {
                    self.selected_activity = selectable.first().copied();
                }
            }
        }
    }
}

impl Default for UiState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TerminalSize {
    pub width: u16,
    pub height: u16,
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
}

#[derive(Debug, PartialEq)]
pub enum AppState {
    Running,
    ShuttingDown,
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum TaskDisplayStatus {
    Pending,
    Running,
    Success,
    Failed,
    Skipped,
    Cancelled,
}

/// Which view is currently active in the TUI
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum ViewMode {
    /// Main activity list view (non-fullscreen, preserves terminal scrollback)
    #[default]
    Main,
    /// Expanded log view for a specific activity (fullscreen, uses alternate screen)
    /// Note: scroll_offset is managed as component-local state for immediate responsiveness
    ExpandedLogs { activity_id: u64 },
}

impl ActivityModel {
    /// Create a new ActivityModel.
    pub fn new() -> Self {
        Self::with_config(Arc::new(TuiConfig::default()))
    }

    /// Create a new ActivityModel with custom configuration.
    pub fn with_config(config: Arc<TuiConfig>) -> Self {
        Self {
            message_log: VecDeque::new(),
            activities: HashMap::new(),
            root_activities: Vec::new(),
            build_logs: HashMap::new(),
            app_state: AppState::Running,
            completed_messages: Vec::new(),
            next_message_id: u64::MAX / 2,
            config,
            done: false,
        }
    }

    /// Returns true if a Done event has been received, signaling work is complete.
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Get the TUI configuration.
    pub fn config(&self) -> &TuiConfig {
        &self.config
    }

    pub fn apply_activity_event(&mut self, event: ActivityEvent) {
        match event {
            ActivityEvent::Build(build_event) => self.handle_build_event(build_event),
            ActivityEvent::Fetch(fetch_event) => self.handle_fetch_event(fetch_event),
            ActivityEvent::Evaluate(eval_event) => self.handle_evaluate_event(eval_event),
            ActivityEvent::Task(task_event) => self.handle_task_event(task_event),
            ActivityEvent::Command(cmd_event) => self.handle_command_event(cmd_event),
            ActivityEvent::Operation(op_event) => self.handle_operation_event(op_event),
            ActivityEvent::Message(msg) => self.handle_message(msg),
            ActivityEvent::Done => self.done = true,
        }
    }

    fn handle_build_event(&mut self, event: Build) {
        match event {
            Build::Queued {
                id,
                name,
                parent,
                derivation_path,
                ..
            } => {
                let variant = ActivityVariant::Build(BuildActivity {
                    phase: Some("queued".to_string()),
                    log_stdout_lines: Vec::new(),
                    log_stderr_lines: Vec::new(),
                });
                self.create_activity_with_state(
                    id,
                    name,
                    parent,
                    derivation_path,
                    variant,
                    ActivityLevel::Info,
                    NixActivityState::Queued,
                );
            }
            Build::Start {
                id,
                name,
                parent,
                derivation_path,
                ..
            } => {
                let variant = ActivityVariant::Build(BuildActivity {
                    phase: Some("running".to_string()),
                    log_stdout_lines: Vec::new(),
                    log_stderr_lines: Vec::new(),
                });
                self.create_activity(id, name, parent, derivation_path, variant, ActivityLevel::Info);
            }
            Build::Complete { id, outcome, .. } => {
                self.handle_activity_complete(id, outcome);
            }
            Build::Phase { id, phase, .. } => {
                self.handle_activity_phase(id, phase);
            }
            Build::Progress {
                id, done, expected, ..
            } => {
                self.handle_item_progress(id, done, expected);
            }
            Build::Log {
                id, line, is_error, ..
            } => {
                self.handle_activity_log(id, line, is_error);
            }
        }
    }

    fn handle_fetch_event(&mut self, event: Fetch) {
        match event {
            Fetch::Start {
                id,
                kind,
                name,
                parent,
                url,
                ..
            } => {
                let substituter = url.as_ref().and_then(|u| {
                    url::Url::parse(u)
                        .ok()
                        .and_then(|parsed| parsed.host_str().map(|h| h.to_string()))
                });
                let variant = match kind {
                    FetchKind::Query => ActivityVariant::Query(QueryActivity {
                        substituter: substituter.clone(),
                    }),
                    FetchKind::Tree => ActivityVariant::FetchTree,
                    FetchKind::Download => ActivityVariant::Download(DownloadActivity {
                        size_current: Some(0),
                        size_total: None,
                        speed: None,
                        substituter,
                    }),
                };
                self.create_activity(id, name, parent, url, variant, ActivityLevel::Info);
            }
            Fetch::Complete { id, outcome, .. } => {
                self.handle_activity_complete(id, outcome);
            }
            Fetch::Progress {
                id, current, total, ..
            } => {
                self.handle_byte_progress(id, current, total);
            }
        }
    }

    fn handle_evaluate_event(&mut self, event: Evaluate) {
        match event {
            Evaluate::Start { id, parent, .. } => {
                let variant = ActivityVariant::Evaluating(EvaluatingActivity::default());
                self.create_activity(id, String::new(), parent, None, variant, ActivityLevel::Info);
            }
            Evaluate::Complete { id, outcome, .. } => {
                self.handle_activity_complete(id, outcome);
            }
            Evaluate::Log { id, line, .. } => {
                // Evaluate logs count files
                self.handle_activity_log(id, line, false);
            }
        }
    }

    fn handle_task_event(&mut self, event: Task) {
        match event {
            Task::Start {
                id,
                name,
                parent,
                detail,
                ..
            } => {
                let variant = ActivityVariant::Task(TaskActivity {
                    status: TaskDisplayStatus::Running,
                    duration: None,
                });
                self.create_activity(id, name, parent, detail, variant, ActivityLevel::Info);
            }
            Task::Complete { id, outcome, .. } => {
                self.handle_activity_complete(id, outcome);
            }
            Task::Progress {
                id, done, expected, ..
            } => {
                self.handle_item_progress(id, done, expected);
            }
            Task::Log {
                id, line, is_error, ..
            } => {
                self.handle_activity_log(id, line, is_error);
            }
        }
    }

    fn handle_command_event(&mut self, event: Command) {
        match event {
            Command::Start {
                id,
                name,
                parent,
                command,
                ..
            } => {
                let variant = ActivityVariant::UserOperation;
                self.create_activity(id, name, parent, command, variant, ActivityLevel::Debug);
            }
            Command::Complete { id, outcome, .. } => {
                self.handle_activity_complete(id, outcome);
            }
            Command::Log {
                id, line, is_error, ..
            } => {
                self.handle_activity_log(id, line, is_error);
            }
        }
    }

    fn handle_operation_event(&mut self, event: Operation) {
        match event {
            Operation::Start {
                id,
                name,
                parent,
                detail,
                level,
                ..
            } => {
                let variant = ActivityVariant::Devenv;
                self.create_activity(id, name, parent, detail, variant, level);
            }
            Operation::Complete { id, outcome, .. } => {
                self.handle_activity_complete(id, outcome);
            }
        }
    }

    fn create_activity(
        &mut self,
        id: u64,
        name: String,
        parent: Option<u64>,
        detail: Option<String>,
        variant: ActivityVariant,
        level: ActivityLevel,
    ) {
        self.create_activity_with_state(id, name, parent, detail, variant, level, NixActivityState::Active);
    }

    fn create_activity_with_state(
        &mut self,
        id: u64,
        name: String,
        parent: Option<u64>,
        detail: Option<String>,
        variant: ActivityVariant,
        level: ActivityLevel,
        state: NixActivityState,
    ) {
        let activity = Activity {
            id,
            name: name.clone(),
            short_name: name,
            parent_id: parent,
            start_time: Instant::now(),
            state,
            completed_at: None,
            detail,
            variant,
            progress: None,
            details: Vec::new(),
            level,
        };

        if parent.is_none() {
            self.root_activities.push(id);
        }

        self.activities.insert(id, activity);
    }

    fn handle_activity_complete(&mut self, id: u64, outcome: ActivityOutcome) {
        // First, get the activity info we need
        let (variant, success, cached, duration) = {
            if let Some(activity) = self.activities.get(&id) {
                let success = matches!(
                    outcome,
                    ActivityOutcome::Success | ActivityOutcome::Cached | ActivityOutcome::Skipped
                );
                let cached = matches!(outcome, ActivityOutcome::Cached);
                let duration = activity.start_time.elapsed();
                (activity.variant.clone(), success, cached, duration)
            } else {
                return;
            }
        };

        // Update the activity state
        if let Some(activity) = self.activities.get_mut(&id) {
            activity.state = NixActivityState::Completed {
                success,
                cached,
                duration,
            };
            activity.completed_at = Some(Instant::now());

            // Clear the phase when build completes so it's not displayed
            if let ActivityVariant::Build(ref mut build) = activity.variant {
                build.phase = None;
            }

            if let ActivityVariant::Task(ref mut task) = activity.variant {
                task.status = match outcome {
                    ActivityOutcome::Success => TaskDisplayStatus::Success,
                    ActivityOutcome::Cached | ActivityOutcome::Skipped => {
                        TaskDisplayStatus::Skipped
                    }
                    ActivityOutcome::Failed | ActivityOutcome::DependencyFailed => {
                        TaskDisplayStatus::Failed
                    }
                    ActivityOutcome::Cancelled => TaskDisplayStatus::Cancelled,
                };
                task.duration = Some(duration);
            }
        }

        // For Devenv (Operation) activities, check if any child Evaluate was cached
        // and propagate that cache status to the parent
        if matches!(variant, ActivityVariant::Devenv) {
            let has_cached_child = self.activities.values().any(|child| {
                child.parent_id == Some(id)
                    && matches!(child.variant, ActivityVariant::Evaluating(_))
                    && matches!(
                        child.state,
                        NixActivityState::Completed { cached: true, .. }
                    )
            });

            if has_cached_child {
                if let Some(activity) = self.activities.get_mut(&id)
                    && let NixActivityState::Completed {
                        success, duration, ..
                    } = activity.state
                {
                    activity.state = NixActivityState::Completed {
                        success,
                        cached: true,
                        duration,
                    };
                }
            }
        }
    }

    fn handle_item_progress(&mut self, id: u64, done: u64, expected: u64) {
        if let Some(activity) = self.activities.get_mut(&id) {
            let percent = if expected > 0 {
                Some((done as f32 / expected as f32) * 100.0)
            } else {
                None
            };

            activity.progress = Some(ProgressActivity {
                current: Some(done),
                total: Some(expected),
                unit: Some("items".to_string()),
                percent,
            });
        }
    }

    fn handle_byte_progress(&mut self, id: u64, current: u64, total: Option<u64>) {
        if let Some(activity) = self.activities.get_mut(&id) {
            let percent = total.map(|t| {
                if t > 0 {
                    (current as f32 / t as f32) * 100.0
                } else {
                    0.0
                }
            });

            activity.progress = Some(ProgressActivity {
                current: Some(current),
                total,
                unit: Some("bytes".to_string()),
                percent,
            });

            if let ActivityVariant::Download(ref mut download) = activity.variant {
                let speed = if let Some(prev_current) = download.size_current {
                    let time_delta = 0.1;
                    let bytes_delta = current.saturating_sub(prev_current) as f64;
                    (bytes_delta / time_delta) as u64
                } else {
                    0
                };

                download.size_current = Some(current);
                download.size_total = total;
                download.speed = Some(speed);
            }
        }
    }

    fn handle_activity_phase(&mut self, id: u64, phase: String) {
        if let Some(activity) = self.activities.get_mut(&id)
            && let ActivityVariant::Build(ref mut build) = activity.variant
        {
            build.phase = Some(phase);
        }
    }

    fn handle_activity_log(&mut self, id: u64, line: String, is_error: bool) {
        let logs = self
            .build_logs
            .entry(id)
            .or_insert_with(|| Arc::new(VecDeque::new()));
        let logs_mut = Arc::make_mut(logs);
        if logs_mut.len() >= self.config.max_log_lines_per_build {
            logs_mut.pop_front();
        }
        logs_mut.push_back(line.clone());

        if let Some(activity) = self.activities.get_mut(&id) {
            match &mut activity.variant {
                ActivityVariant::Build(build) => {
                    if is_error {
                        build.log_stderr_lines.push(line);
                    } else {
                        build.log_stdout_lines.push(line);
                    }
                }
                ActivityVariant::Evaluating(eval) => {
                    eval.files_evaluated += 1;
                }
                _ => {}
            }
        }
    }

    fn handle_message(&mut self, msg: Message) {
        self.add_log_message(msg.clone());

        // Only create activity for messages with a parent
        if msg.parent.is_some() {
            let id = self.next_message_id;
            self.next_message_id += 1;

            let level = msg.level;
            let has_details = msg.details.is_some();
            let variant = ActivityVariant::Message(MessageActivity {
                level,
                details: msg.details.clone(),
            });
            self.create_activity(id, msg.text, msg.parent, None, variant, level);

            // Store details as lines in build_logs for expansion
            if let Some(details) = msg.details {
                let lines: VecDeque<String> = details.lines().map(String::from).collect();
                self.build_logs.insert(id, Arc::new(lines));
            }

            // Mark message activities as immediately completed (they're just informational)
            if let Some(activity) = self.activities.get_mut(&id) {
                activity.state = NixActivityState::Completed {
                    success: has_details, // Show as "expandable" if has details
                    cached: false,
                    duration: std::time::Duration::ZERO,
                };
                activity.completed_at = Some(Instant::now());
            }
        }
    }

    pub fn add_log_message(&mut self, message: Message) {
        self.message_log.push_back(message);
        if self.message_log.len() > self.config.max_log_messages {
            self.message_log.pop_front();
        }
    }

    pub fn get_active_activities(&self) -> Vec<&Activity> {
        self.activities
            .values()
            .filter(|activity| {
                matches!(
                    activity.state,
                    NixActivityState::Queued | NixActivityState::Active
                )
            })
            .collect()
    }

    pub fn get_selectable_activity_ids(&self) -> Vec<u64> {
        self.get_display_activities()
            .into_iter()
            .filter(|da| {
                matches!(
                    da.activity.variant,
                    ActivityVariant::Build(_) | ActivityVariant::Evaluating(_)
                )
            })
            .map(|da| da.activity.id)
            .collect()
    }

    pub fn get_display_activities(&self) -> Vec<DisplayActivity> {
        self.get_display_activities_with_limit(&ChildActivityLimit::default())
    }

    pub fn get_display_activities_with_limit(
        &self,
        limit: &ChildActivityLimit,
    ) -> Vec<DisplayActivity> {
        let mut activities = Vec::new();
        let mut processed = std::collections::HashSet::new();

        for &root_id in &self.root_activities {
            self.add_display_activity(&mut activities, root_id, 0, &mut processed, limit);
        }

        activities
    }

    fn add_display_activity(
        &self,
        activities: &mut Vec<DisplayActivity>,
        activity_id: u64,
        depth: usize,
        processed: &mut std::collections::HashSet<u64>,
        limit: &ChildActivityLimit,
    ) {
        if !processed.insert(activity_id) {
            return;
        }

        if let Some(activity) = self.activities.get(&activity_id) {
            // Skip command activities (UserOperation) - they are internal details
            if matches!(activity.variant, ActivityVariant::UserOperation) {
                return;
            }

            // Filter by activity level: skip activities below the filter level
            // ActivityLevel ordering: Error < Warn < Info < Debug < Trace
            // We show activities at or below (more severe than or equal to) filter_level
            if activity.level > self.config.filter_level {
                return;
            }

            // Get visible children with limiting (only applies if this activity has children)
            let (visible_children, _total, _hidden_count) =
                self.get_visible_children(activity_id, limit);

            activities.push(DisplayActivity {
                activity: activity.clone(),
                depth,
            });

            // Recursively add visible children
            for child in visible_children {
                self.add_display_activity(activities, child.id, depth + 1, processed, limit);
            }
        }
    }

    pub fn calculate_summary(&self) -> ActivitySummary {
        let mut summary = ActivitySummary::default();

        for activity in self.activities.values() {
            match (&activity.variant, &activity.state) {
                (ActivityVariant::Build(_), NixActivityState::Queued | NixActivityState::Active) => {
                    summary.active_builds += 1
                }
                (ActivityVariant::Build(_), NixActivityState::Completed { success: true, .. }) => {
                    summary.completed_builds += 1;
                }
                (ActivityVariant::Build(_), NixActivityState::Completed { success: false, .. }) => {
                    summary.failed_builds += 1;
                }
                (
                    ActivityVariant::Download(_),
                    NixActivityState::Queued | NixActivityState::Active,
                ) => summary.active_downloads += 1,
                (
                    ActivityVariant::Download(_),
                    NixActivityState::Completed { success: true, .. },
                ) => {
                    summary.completed_downloads += 1;
                }
                (
                    ActivityVariant::Query(_),
                    NixActivityState::Queued | NixActivityState::Active,
                ) => summary.active_queries += 1,
                (ActivityVariant::Query(_), NixActivityState::Completed { success: true, .. }) => {
                    summary.completed_queries += 1;
                }
                (ActivityVariant::Task(task), _) => match task.status {
                    TaskDisplayStatus::Running | TaskDisplayStatus::Pending => {
                        summary.running_tasks += 1
                    }
                    TaskDisplayStatus::Success | TaskDisplayStatus::Skipped => {
                        summary.completed_tasks += 1
                    }
                    TaskDisplayStatus::Failed | TaskDisplayStatus::Cancelled => {
                        summary.failed_tasks += 1
                    }
                },
                _ => {}
            }
        }

        summary.total_builds =
            summary.active_builds + summary.completed_builds + summary.failed_builds;
        summary
    }

    pub fn get_activity(&self, activity_id: u64) -> Option<&Activity> {
        self.activities.get(&activity_id)
    }

    pub fn get_build_logs(&self, activity_id: u64) -> Option<&Arc<VecDeque<String>>> {
        self.build_logs.get(&activity_id)
    }

    /// Get standalone error messages (those without a parent activity).
    /// Returns the most recent error messages for display.
    pub fn get_error_messages(&self) -> Vec<&Message> {
        self.message_log
            .iter()
            .filter(|msg| msg.parent.is_none() && msg.level == ActivityLevel::Error)
            .collect()
    }

    pub fn get_total_duration(&self) -> Option<std::time::Duration> {
        let earliest_start = self.activities.values().map(|a| a.start_time).min()?;
        Some(Instant::now().duration_since(earliest_start))
    }

    pub fn get_active_display_activities(&self) -> Vec<DisplayActivity> {
        self.get_display_activities()
            .into_iter()
            .filter(|da| {
                matches!(
                    da.activity.state,
                    NixActivityState::Queued | NixActivityState::Active
                )
            })
            .collect()
    }

    /// Get children of an activity, limited by the provided config.
    /// Prioritizes active activities, then lingering completed ones, then older completed ones.
    /// Returns a tuple of (visible_children, total_children_count, hidden_count).
    ///
    /// Task activities always show all their children without linger/limit restrictions,
    /// so completed tasks remain visible after execution.
    pub fn get_visible_children(
        &self,
        parent_id: u64,
        limit: &ChildActivityLimit,
    ) -> (Vec<&Activity>, usize, usize) {
        let now = Instant::now();

        // Check if parent is a Task activity - tasks always show all children
        let parent_is_task = self
            .activities
            .get(&parent_id)
            .is_some_and(|a| matches!(a.variant, ActivityVariant::Task(_)));

        // Get all children of this parent, excluding UserOperation and filtered by level
        let filter_level = self.config.filter_level;
        let mut all_children: Vec<_> = self
            .activities
            .values()
            .filter(|a| {
                a.parent_id == Some(parent_id)
                    && !matches!(a.variant, ActivityVariant::UserOperation)
                    && a.level <= filter_level
            })
            .collect();

        let total_count = all_children.len();

        // Sort by id for consistent ordering
        all_children.sort_by_key(|a| a.id);

        // For Task parents, always show all children without limits
        if parent_is_task {
            return (all_children, total_count, 0);
        }

        // Partition into active (including queued) and completed
        let (active, completed): (Vec<_>, Vec<_>) = all_children.into_iter().partition(|a| {
            matches!(
                a.state,
                NixActivityState::Queued | NixActivityState::Active
            )
        });

        // Sort completed by completion time (most recent first)
        let mut completed_with_time: Vec<_> = completed
            .into_iter()
            .map(|a| {
                let completed_at = a.completed_at.unwrap_or(a.start_time);
                (a, completed_at)
            })
            .collect();
        completed_with_time.sort_by(|a, b| b.1.cmp(&a.1)); // Most recent first

        // Separate lingering (within linger_duration) from older completed
        let (lingering, older): (Vec<_>, Vec<_>) =
            completed_with_time
                .into_iter()
                .partition(|(_, completed_at)| {
                    now.duration_since(*completed_at) < limit.linger_duration
                });

        // Build result: prioritize active, then lingering, then older
        let mut result: Vec<&Activity> = Vec::new();

        // Add all active items first (they always show)
        for a in &active {
            if result.len() >= limit.max_lines {
                break;
            }
            result.push(a);
        }

        // Add lingering completed items
        for (a, _) in &lingering {
            if result.len() >= limit.max_lines {
                break;
            }
            result.push(a);
        }

        // Fill remaining with older completed items
        for (a, _) in &older {
            if result.len() >= limit.max_lines {
                break;
            }
            result.push(a);
        }

        // Sort final result by id for consistent display order
        result.sort_by_key(|a| a.id);

        let hidden_count = total_count.saturating_sub(result.len());
        (result, total_count, hidden_count)
    }

    /// Check if an activity has any children
    pub fn has_children(&self, activity_id: u64) -> bool {
        self.activities
            .values()
            .any(|a| a.parent_id == Some(activity_id))
    }

    /// Get count of children for an activity (respecting filter level)
    pub fn get_children_count(&self, activity_id: u64) -> usize {
        let filter_level = self.config.filter_level;
        self.activities
            .values()
            .filter(|a| {
                a.parent_id == Some(activity_id)
                    && !matches!(a.variant, ActivityVariant::UserOperation)
                    && a.level <= filter_level
            })
            .count()
    }

    /// Calculate the height that the TUI will render.
    /// This mirrors the height calculation in view.rs.
    pub fn calculate_rendered_height(
        &self,
        selected_activity: Option<u64>,
        terminal_height: u16,
    ) -> u16 {
        let activities = self.get_display_activities();

        let mut total_height: usize = 0;

        for display_activity in activities.iter() {
            total_height += 1; // Base height for activity

            let is_selected = selected_activity.is_some_and(|id| {
                display_activity.activity.id == id && display_activity.activity.id != 0
            });

            // Add extra line for downloads with progress
            if let ActivityVariant::Download(ref download_data) = display_activity.activity.variant
            {
                if download_data.size_current.is_some() && download_data.size_total.is_some() {
                    total_height += 1;
                } else if let Some(progress) = &display_activity.activity.progress
                    && progress.total.unwrap_or(0) > 0
                {
                    total_height += 1;
                }
            }

            // Build and evaluation activities show logs when selected
            if is_selected
                && (matches!(display_activity.activity.variant, ActivityVariant::Build(_))
                    || matches!(
                        display_activity.activity.variant,
                        ActivityVariant::Evaluating(_)
                    ))
                && let Some(logs) = self.get_build_logs(display_activity.activity.id)
            {
                let visible_count = logs.len().min(10); // LOG_VIEWPORT_COLLAPSED = 10
                total_height += visible_count.max(1);
            }

            // Message activities with details
            if let ActivityVariant::Message(ref msg_data) = display_activity.activity.variant
                && msg_data.details.is_some()
                && let Some(logs) = self.get_build_logs(display_activity.activity.id)
            {
                let visible_count = logs.len().min(10);
                total_height += visible_count;
            }
        }

        // Apply minimum height for activities (mirrors view.rs)
        let min_height = 3;
        let dynamic_height = total_height.max(min_height);

        // Error messages panel
        let error_count = self.get_error_messages().len().min(10);

        // Total: dynamic_height + error panel + summary line + buffer
        let calculated = (dynamic_height + error_count + 2) as u16;

        // Clamp to terminal height
        calculated.min(terminal_height)
    }
}

/// State of a Nix activity
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NixActivityState {
    /// Activity is queued, waiting to start (no timer shown)
    Queued,
    /// Activity is actively running (timer shown)
    Active,
    Completed {
        success: bool,
        cached: bool,
        duration: std::time::Duration,
    },
}

#[derive(Debug)]
pub struct DisplayActivity {
    pub activity: Activity,
    pub depth: usize,
}

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
    pub running_tasks: usize,
    pub completed_tasks: usize,
    pub failed_tasks: usize,
}
