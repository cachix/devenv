use crate::{LogMessage, LogSource, NixActivityState, OperationId};
use devenv_activity::{
    ActivityEvent, ActivityOutcome, Build, Command, Evaluate, Fetch, FetchKind, Message,
    Operation, Task,
};
use std::collections::{HashMap, VecDeque};
use std::time::Instant;

const MAX_LOG_MESSAGES: usize = 1000;
const MAX_LOG_LINES_PER_BUILD: usize = 1000;

#[derive(Debug)]
pub struct Model {
    pub message_log: VecDeque<LogMessage>,
    pub activities: HashMap<u64, Activity>,
    pub root_activities: Vec<u64>,
    pub build_logs: HashMap<u64, VecDeque<String>>,
    pub ui: UiState,
    pub app_state: AppState,
    pub completed_messages: Vec<String>,
}

impl Default for Model {
    fn default() -> Self {
        Self::new()
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
    pub operation_id: OperationId,
    pub name: String,
    pub short_name: String,
    pub parent_id: Option<u64>,
    pub start_time: Instant,
    pub state: NixActivityState,
    pub detail: Option<String>,
    pub variant: ActivityVariant,
    pub progress: Option<ProgressActivity>,
    /// Additional details/metadata (shown when expanded)
    pub details: Vec<ActivityDetail>,
}

#[derive(Debug)]
pub struct UiState {
    pub spinner_frame: usize,
    pub last_spinner_update: Instant,
    pub viewport: ViewportConfig,
    pub selected_activity: Option<u64>,
    pub scroll: ScrollState,
    pub view_options: ViewOptions,
    pub terminal_size: TerminalSize,
}

#[derive(Debug, Clone, Copy)]
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
    pub show_expanded_logs: bool,
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

impl Model {
    /// Create a new Model, querying the terminal for its size.
    pub fn new() -> Self {
        let (width, height) = crossterm::terminal::size().unwrap_or((80, 24));
        Self::with_terminal_size(width, height)
    }

    /// Create a new Model with a fixed terminal size (useful for tests).
    pub fn with_terminal_size(width: u16, height: u16) -> Self {
        Self {
            message_log: VecDeque::new(),
            activities: HashMap::new(),
            root_activities: Vec::new(),
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
                terminal_size: TerminalSize { width, height },
            },
            app_state: AppState::Running,
            completed_messages: Vec::new(),
        }
    }

    /// Update terminal size (call on resize events).
    pub fn set_terminal_size(&mut self, width: u16, height: u16) {
        self.ui.terminal_size = TerminalSize { width, height };
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
        }
    }

    fn handle_build_event(&mut self, event: Build) {
        match event {
            Build::Start {
                id,
                name,
                parent,
                derivation_path,
                ..
            } => {
                let variant = ActivityVariant::Build(BuildActivity {
                    phase: Some("preparing".to_string()),
                    log_stdout_lines: Vec::new(),
                    log_stderr_lines: Vec::new(),
                });
                self.create_activity(id, name, parent, derivation_path, variant);
            }
            Build::Complete { id, outcome, .. } => {
                self.handle_activity_complete(id, outcome);
            }
            Build::Phase { id, phase, .. } => {
                self.handle_activity_phase(id, phase);
            }
            Build::Progress {
                id,
                done,
                expected,
                ..
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
                    url::Url::parse(u).ok().and_then(|parsed| {
                        parsed.host_str().map(|h| h.to_string())
                    })
                });
                let variant = match kind {
                    FetchKind::Query => ActivityVariant::Query(QueryActivity { substituter }),
                    FetchKind::Tree => ActivityVariant::FetchTree,
                    FetchKind::Download => ActivityVariant::Download(DownloadActivity {
                        size_current: Some(0),
                        size_total: None,
                        speed: None,
                        substituter: None,
                    }),
                };
                self.create_activity(id, name, parent, url, variant);
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
            Evaluate::Start {
                id, name, parent, ..
            } => {
                let variant = ActivityVariant::Evaluating(EvaluatingActivity::default());
                self.create_activity(id, name, parent, None, variant);
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
                self.create_activity(id, name, parent, detail, variant);
            }
            Task::Complete { id, outcome, .. } => {
                self.handle_activity_complete(id, outcome);
            }
            Task::Progress {
                id,
                done,
                expected,
                ..
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
                self.create_activity(id, name, parent, command, variant);
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
                ..
            } => {
                let variant = ActivityVariant::Devenv;
                self.create_activity(id, name, parent, detail, variant);
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
    ) {
        let activity = Activity {
            id,
            operation_id: OperationId::from_activity_id(id),
            name: name.clone(),
            short_name: name,
            parent_id: parent,
            start_time: Instant::now(),
            state: NixActivityState::Active,
            detail,
            variant,
            progress: None,
            details: Vec::new(),
        };

        if parent.is_none() {
            self.root_activities.push(id);
        }

        self.activities.insert(id, activity);
    }

    fn handle_activity_complete(&mut self, id: u64, outcome: ActivityOutcome) {
        if let Some(activity) = self.activities.get_mut(&id) {
            let success = matches!(outcome, ActivityOutcome::Success);
            let duration = activity.start_time.elapsed();
            activity.state = NixActivityState::Completed { success, duration };

            if let ActivityVariant::Task(ref mut task) = activity.variant {
                task.status = if success {
                    TaskDisplayStatus::Success
                } else {
                    TaskDisplayStatus::Failed
                };
                task.duration = Some(duration);
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
        let logs = self.build_logs.entry(id).or_default();
        if logs.len() >= MAX_LOG_LINES_PER_BUILD {
            logs.pop_front();
        }
        logs.push_back(line.clone());

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
        let log_msg = LogMessage::new(msg.level.into(), msg.text, LogSource::System, HashMap::new());
        self.add_log_message(log_msg);
    }

    pub fn add_log_message(&mut self, message: LogMessage) {
        self.message_log.push_back(message);
        if self.message_log.len() > MAX_LOG_MESSAGES {
            self.message_log.pop_front();
        }
    }

    pub fn get_active_activities(&self) -> Vec<&Activity> {
        self.activities
            .values()
            .filter(|activity| matches!(activity.state, NixActivityState::Active))
            .collect()
    }

    pub fn get_selectable_activity_ids(&self) -> Vec<u64> {
        self.activities
            .iter()
            .filter(|(_, activity)| {
                matches!(activity.state, NixActivityState::Active)
                    && matches!(
                        activity.variant,
                        ActivityVariant::Build(_) | ActivityVariant::Evaluating(_)
                    )
            })
            .map(|(id, _)| *id)
            .collect()
    }

    pub fn select_next_activity(&mut self) {
        let selectable = self.get_selectable_activity_ids();
        if !selectable.is_empty() {
            match self.ui.selected_activity {
                None => {
                    self.ui.selected_activity = selectable.first().copied();
                }
                Some(current_id) => {
                    if let Some(current_pos) = selectable.iter().position(|&id| id == current_id) {
                        let next_pos = (current_pos + 1) % selectable.len();
                        self.ui.selected_activity = Some(selectable[next_pos]);
                    } else {
                        self.ui.selected_activity = selectable.first().copied();
                    }
                }
            }
        }
    }

    pub fn select_previous_activity(&mut self) {
        let selectable = self.get_selectable_activity_ids();
        if !selectable.is_empty() {
            match self.ui.selected_activity {
                None => {
                    self.ui.selected_activity = selectable.last().copied();
                }
                Some(current_id) => {
                    if let Some(current_pos) = selectable.iter().position(|&id| id == current_id) {
                        let prev_pos = if current_pos == 0 {
                            selectable.len() - 1
                        } else {
                            current_pos - 1
                        };
                        self.ui.selected_activity = Some(selectable[prev_pos]);
                    } else {
                        self.ui.selected_activity = selectable.last().copied();
                    }
                }
            }
        }
    }

    pub fn get_display_activities(&self) -> Vec<DisplayActivity> {
        let mut activities = Vec::new();
        let mut processed = std::collections::HashSet::new();

        for &root_id in &self.root_activities {
            self.add_display_activity(&mut activities, root_id, 0, &mut processed);
        }

        activities
    }

    fn add_display_activity(
        &self,
        activities: &mut Vec<DisplayActivity>,
        activity_id: u64,
        depth: usize,
        processed: &mut std::collections::HashSet<u64>,
    ) {
        if !processed.insert(activity_id) {
            return;
        }

        if let Some(activity) = self.activities.get(&activity_id) {
            // Skip command activities (UserOperation) - they are internal details
            if matches!(activity.variant, ActivityVariant::UserOperation) {
                return;
            }

            activities.push(DisplayActivity {
                activity: activity.clone(),
                depth,
            });

            let mut children: Vec<_> = self
                .activities
                .values()
                .filter(|child| child.parent_id == Some(activity_id))
                .collect();
            children.sort_by_key(|c| c.id);

            for child in children {
                self.add_display_activity(activities, child.id, depth + 1, processed);
            }
        }
    }

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

    pub fn get_selected_activity(&self) -> Option<&Activity> {
        self.ui
            .selected_activity
            .and_then(|id| self.activities.get(&id))
    }

    pub fn get_build_logs(&self, activity_id: u64) -> Option<&VecDeque<String>> {
        self.build_logs.get(&activity_id)
    }

    pub fn get_total_duration(&self) -> Option<std::time::Duration> {
        let earliest_start = self.activities.values().map(|a| a.start_time).min()?;
        Some(Instant::now().duration_since(earliest_start))
    }

    pub fn get_active_display_activities(&self) -> Vec<DisplayActivity> {
        self.get_display_activities()
            .into_iter()
            .filter(|da| matches!(da.activity.state, NixActivityState::Active))
            .collect()
    }
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
}
