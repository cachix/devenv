use crate::app::TuiConfig;
use devenv_activity::{
    ActivityEvent, ActivityLevel, ActivityOutcome, Build, Command, EvalOp, Evaluate,
    ExpectedCategory, Fetch, FetchKind, Message, Operation, Process, ProcessStatus, SetExpected,
    Task,
};
use std::collections::{HashMap, HashSet, VecDeque};
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
    /// Total count of log lines received per activity (not affected by buffer rotation)
    pub log_line_counts: HashMap<u64, usize>,
    pub app_state: AppState,
    pub completed_messages: Vec<String>,
    config: Arc<TuiConfig>,
    /// Expected build count announced by Nix (via SetExpected events)
    expected_builds: Option<u64>,
    /// Expected download count announced by Nix (via SetExpected events)
    expected_downloads: Option<u64>,
    /// Additional parents for activities (for displaying under multiple parents in TUI)
    additional_parents: HashMap<u64, Vec<u64>>,
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
    pub show_output: bool,
    pub last_log_line: Option<String>,
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
pub struct ProcessActivity {
    pub status: ProcessStatus,
    pub ports: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ActivityVariant {
    Task(TaskActivity),
    UserOperation,
    Evaluating(EvaluatingActivity),
    Build(BuildActivity),
    Download(DownloadActivity),
    /// Copying local sources to the store
    Copy,
    Query(QueryActivity),
    FetchTree,
    /// Devenv-specific operations (e.g., "Building shell", "Entering shell")
    Devenv,
    /// Long-running managed processes
    Process(ProcessActivity),
    /// Standalone messages displayed as children of their parent activity
    Message(MessageActivity),
    Unknown,
}

impl ActivityVariant {
    /// Whether this variant is always rendered as a top-level activity in the TUI,
    /// even when it has a tracing parent.
    /// These activities are added to `root_activities` and excluded from their
    /// parent's child list.
    fn is_always_top_level(&self) -> bool {
        matches!(self, ActivityVariant::Devenv)
    }
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
    pub last_render_height: u16,
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
            last_render_height: 0,
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
                self.selected_activity = selectable.last().copied();
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

/// Controls rendering behavior independent of view content.
/// This is passed to the view function to control how it renders,
/// separate from ViewMode which controls *what* to render.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum RenderContext {
    /// Normal rendering with full UI (nav bar, etc.)
    #[default]
    Normal,
    /// Final render before exit - no nav bar
    Final,
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
            log_line_counts: HashMap::new(),
            app_state: AppState::Running,
            completed_messages: Vec::new(),
            config,
            expected_builds: None,
            expected_downloads: None,
            additional_parents: HashMap::new(),
        }
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
            ActivityEvent::Process(proc_event) => self.handle_process_event(proc_event),
            ActivityEvent::Operation(op_event) => self.handle_operation_event(op_event),
            ActivityEvent::Message(msg) => self.handle_message(msg),
            ActivityEvent::SetExpected(expected) => self.handle_set_expected(expected),
            ActivityEvent::Shell(_) => {
                // Shell events are handled separately by the shell runner
                // They don't affect the activity model
            }
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
                self.create_activity_with_options(
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
                self.create_activity(
                    id,
                    name,
                    parent,
                    derivation_path,
                    variant,
                    ActivityLevel::Info,
                );
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
                // Skip .narinfo downloads - these are redundant with Query activities.
                // When Nix checks if a store path exists in a cache, it emits both:
                // 1. A Query activity with the human-readable store path name
                // 2. A Download activity for the actual .narinfo HTTP request
                // We only display the Query since it has the better name.
                if kind == FetchKind::Download && name.ends_with(".narinfo") {
                    return;
                }

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
                    FetchKind::Copy => ActivityVariant::Copy,
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
            Evaluate::Start {
                id,
                name,
                level,
                parent,
                ..
            } => {
                let variant = ActivityVariant::Evaluating(EvaluatingActivity::default());
                self.create_activity(id, name, parent, None, variant, level);
            }
            Evaluate::Complete { id, outcome, .. } => {
                self.handle_activity_complete(id, outcome);
            }
            Evaluate::Log { id, line, .. } => {
                // Evaluate logs count files
                self.handle_activity_log(id, line, false);
            }
            Evaluate::Op { id, op, .. } => {
                // Log the op to the activity's log buffer
                let line = format_eval_op(&op);
                self.log_to_activity(id, line);

                // Count file read operations for the progress display.
                // Excludes CopiedSource, ReadDir, GetEnv, TrackedPath.
                let is_file_read = matches!(
                    op,
                    EvalOp::EvaluatedFile { .. }
                        | EvalOp::ReadFile { .. }
                        | EvalOp::PathExists { .. }
                );
                if is_file_read
                    && let Some(activity) = self.activities.get_mut(&id)
                    && let ActivityVariant::Evaluating(eval) = &mut activity.variant
                {
                    eval.files_evaluated += 1;
                }
            }
        }
    }

    fn handle_task_event(&mut self, event: Task) {
        match event {
            Task::Hierarchy { tasks, edges, .. } => {
                // Build parent relationships from edges first
                // edges are (parent_id, child_id) pairs
                // First edge for a child is the primary parent, rest are additional parents
                let mut primary_parents: HashMap<u64, u64> = HashMap::new();
                let mut additional: HashMap<u64, Vec<u64>> = HashMap::new();

                for (parent_id, child_id) in edges {
                    if let std::collections::hash_map::Entry::Vacant(e) =
                        primary_parents.entry(child_id)
                    {
                        // First edge - set as primary parent
                        e.insert(parent_id);
                    } else {
                        // Already has primary parent, add as additional
                        additional.entry(child_id).or_default().push(parent_id);
                    }
                }

                // Create all task activities upfront in Queued state
                // Use parent from edges if available
                for task_info in tasks {
                    // Skip process tasks - they create Process activities instead
                    if task_info.is_process {
                        continue;
                    }

                    let variant = ActivityVariant::Task(TaskActivity {
                        status: TaskDisplayStatus::Pending,
                        duration: None,
                        show_output: task_info.show_output,
                        last_log_line: None,
                    });
                    let parent = primary_parents.get(&task_info.id).copied();
                    self.create_activity_with_options(
                        task_info.id,
                        task_info.name,
                        parent,
                        None,
                        variant,
                        ActivityLevel::Info,
                        NixActivityState::Queued,
                    );
                }

                // Store additional parents
                for (child_id, parents) in additional {
                    if !parents.is_empty() {
                        self.additional_parents.insert(child_id, parents);
                    }
                }
            }
            Task::Start { id, .. } => {
                // Transition task from Queued to Active
                if let Some(activity) = self.activities.get_mut(&id) {
                    activity.state = NixActivityState::Active;
                    activity.start_time = Instant::now();
                    activity.completed_at = None;
                    if let ActivityVariant::Task(task) = &mut activity.variant {
                        task.status = TaskDisplayStatus::Running;
                        task.duration = None;
                    }
                }
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

    fn handle_process_event(&mut self, event: Process) {
        match event {
            Process::Start {
                id,
                name,
                parent,
                command,
                ports,
                level,
                ..
            } => {
                let variant = ActivityVariant::Process(ProcessActivity {
                    status: ProcessStatus::Running,
                    ports,
                });
                self.create_activity(id, name, parent, command, variant, level);
            }
            Process::Complete { id, outcome, .. } => {
                // Update status to Stopped before completing
                if let Some(activity) = self.activities.get_mut(&id)
                    && let ActivityVariant::Process(ref mut proc) = activity.variant
                {
                    proc.status = ProcessStatus::Stopped;
                }
                self.handle_activity_complete(id, outcome);
            }
            Process::Log {
                id, line, is_error, ..
            } => {
                self.handle_activity_log(id, line, is_error);
            }
            Process::Status { id, status, .. } => {
                if let Some(activity) = self.activities.get_mut(&id) {
                    if let ActivityVariant::Process(ref mut proc) = activity.variant {
                        proc.status = status;
                    }
                    // Restarting status reactivates a failed/completed process
                    if matches!(status, ProcessStatus::Restarting) {
                        activity.state = NixActivityState::Active;
                        activity.completed_at = None;
                    }
                }
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
            Operation::Progress {
                id,
                done,
                expected,
                detail,
                ..
            } => {
                self.handle_item_progress(id, done, expected);
                if let Some(activity) = self.activities.get_mut(&id) {
                    activity.detail = detail;
                }
            }
            Operation::Log {
                id, line, is_error, ..
            } => {
                self.handle_activity_log(id, line, is_error);
            }
        }
    }

    fn handle_set_expected(&mut self, event: SetExpected) {
        match event.category {
            ExpectedCategory::Build => {
                // Accumulate expected builds
                self.expected_builds = Some(self.expected_builds.unwrap_or(0) + event.expected);
            }
            ExpectedCategory::Download => {
                // Accumulate expected downloads
                self.expected_downloads =
                    Some(self.expected_downloads.unwrap_or(0) + event.expected);
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
        self.create_activity_with_options(
            id,
            name,
            parent,
            detail,
            variant,
            level,
            NixActivityState::Active,
        );
    }

    fn create_activity_with_options(
        &mut self,
        id: u64,
        name: String,
        parent: Option<u64>,
        detail: Option<String>,
        variant: ActivityVariant,
        level: ActivityLevel,
        state: NixActivityState,
    ) {
        // Nix stream activities (Build, Fetch, Evaluate) don't have explicit levels
        // in their events - they come from Nix's JSON output. We inherit level from
        // parent so that activities under a Debug-level operation are also Debug.
        // Our own activities (Operation, Task, Command, Message) already have correct
        // levels set explicitly, so no inheritance needed.
        let is_nix_activity = matches!(
            variant,
            ActivityVariant::Build(_)
                | ActivityVariant::Download(_)
                | ActivityVariant::Copy
                | ActivityVariant::Evaluating(_)
                | ActivityVariant::Query(_)
                | ActivityVariant::FetchTree
        );

        let effective_level = if is_nix_activity {
            // Inherit level from parent if parent has a higher (less visible) level
            if let Some(parent_id) = parent {
                if let Some(parent_activity) = self.activities.get(&parent_id) {
                    if parent_activity.level > level {
                        parent_activity.level
                    } else {
                        level
                    }
                } else {
                    level
                }
            } else {
                level
            }
        } else {
            // Non-Nix activities have their own explicit levels
            level
        };

        let is_root = parent.is_none() || variant.is_always_top_level();

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
            level: effective_level,
        };

        if is_root {
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
                let duration = if matches!(&activity.state, NixActivityState::Queued) {
                    Duration::ZERO
                } else {
                    activity.start_time.elapsed()
                };
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

            if has_cached_child
                && let Some(activity) = self.activities.get_mut(&id)
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

        // Track total line count (not affected by buffer rotation)
        *self.log_line_counts.entry(id).or_insert(0) += 1;

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
                ActivityVariant::Task(task) => {
                    task.last_log_line = Some(line);
                }
                _ => {}
            }
        }
    }

    /// Log a line to an activity's log buffer without counting or other side effects.
    fn log_to_activity(&mut self, id: u64, line: String) {
        let logs = self
            .build_logs
            .entry(id)
            .or_insert_with(|| Arc::new(VecDeque::new()));
        let logs_mut = Arc::make_mut(logs);
        if logs_mut.len() >= self.config.max_log_lines_per_build {
            logs_mut.pop_front();
        }
        logs_mut.push_back(line);

        *self.log_line_counts.entry(id).or_insert(0) += 1;
    }

    fn handle_message(&mut self, msg: Message) {
        self.add_log_message(msg.clone());

        // Only create activity for messages with a parent
        if msg.parent.is_some() {
            let id = msg.id;
            let level = msg.level;
            let has_details = msg.details.is_some();
            let variant = ActivityVariant::Message(MessageActivity {
                level,
                details: msg.details.clone(),
            });
            self.create_activity_with_options(
                id,
                msg.text,
                msg.parent,
                None,
                variant,
                level,
                NixActivityState::Active,
            );

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
        let mut seen = HashSet::new();
        self.get_display_activities()
            .into_iter()
            .filter(|da| {
                self.build_logs
                    .get(&da.activity.id)
                    .is_some_and(|logs| !logs.is_empty())
            })
            .filter_map(|da| {
                let id = da.activity.id;
                seen.insert(id).then_some(id)
            })
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
        // Track (activity_id, parent_id) pairs to allow same activity under multiple parents
        let mut processed: std::collections::HashSet<(u64, Option<u64>)> =
            std::collections::HashSet::new();

        for &root_id in &self.root_activities {
            self.add_display_activity(&mut activities, root_id, None, 0, &mut processed, limit);
        }

        activities
    }

    fn add_display_activity(
        &self,
        activities: &mut Vec<DisplayActivity>,
        activity_id: u64,
        parent_id: Option<u64>,
        depth: usize,
        processed: &mut std::collections::HashSet<(u64, Option<u64>)>,
        limit: &ChildActivityLimit,
    ) {
        // Track (activity_id, parent_id) to allow same activity under different parents
        if !processed.insert((activity_id, parent_id)) {
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
            let activity_visible = activity.level <= self.config.filter_level;

            // Get all children (not filtered by level) so we can traverse through
            // filtered parents to find visible children
            let (all_children, _total, _hidden_count) = self.get_children(activity_id, limit);

            if activity_visible {
                activities.push(DisplayActivity {
                    activity: activity.clone(),
                    depth,
                });
            }

            // Recursively process children.
            // Even if this activity is filtered, still traverse to children
            // so we can find visible descendants (e.g., Info messages under Debug parents).
            let child_depth = if activity_visible { depth + 1 } else { depth };
            for child in all_children {
                self.add_display_activity(
                    activities,
                    child.id,
                    Some(activity_id),
                    child_depth,
                    processed,
                    limit,
                );
            }
        }
    }

    pub fn calculate_summary(&self) -> ActivitySummary {
        let mut summary = ActivitySummary::default();

        for activity in self.activities.values() {
            match (&activity.variant, &activity.state) {
                (
                    ActivityVariant::Build(_),
                    NixActivityState::Queued | NixActivityState::Active,
                ) => summary.active_builds += 1,
                (ActivityVariant::Build(_), NixActivityState::Completed { success: true, .. }) => {
                    summary.completed_builds += 1;
                }
                (ActivityVariant::Build(_), NixActivityState::Completed { success: false, .. }) => {
                    summary.failed_builds += 1;
                }
                (
                    ActivityVariant::Download(_) | ActivityVariant::Copy,
                    NixActivityState::Queued | NixActivityState::Active,
                ) => summary.active_downloads += 1,
                (
                    ActivityVariant::Download(_) | ActivityVariant::Copy,
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
                (ActivityVariant::Process(proc), NixActivityState::Active) => {
                    if proc.status == ProcessStatus::Running || proc.status == ProcessStatus::Ready
                    {
                        summary.running_processes += 1;
                    }
                }
                _ => {}
            }
        }

        summary.total_builds =
            summary.active_builds + summary.completed_builds + summary.failed_builds;
        summary.expected_builds = self.expected_builds;
        summary.expected_downloads = self.expected_downloads;
        summary
    }

    pub fn get_activity(&self, activity_id: u64) -> Option<&Activity> {
        self.activities.get(&activity_id)
    }

    pub fn get_build_logs(&self, activity_id: u64) -> Option<&Arc<VecDeque<String>>> {
        self.build_logs.get(&activity_id)
    }

    /// Get total log line count for an activity (not affected by buffer rotation)
    pub fn get_log_line_count(&self, activity_id: u64) -> usize {
        self.log_line_counts.get(&activity_id).copied().unwrap_or(0)
    }

    /// Get standalone error messages (those without a parent activity).
    /// Returns the most recent error messages for display in the TUI panel.
    pub fn get_error_messages(&self) -> Vec<&Message> {
        self.message_log
            .iter()
            .filter(|msg| msg.parent.is_none() && msg.level == ActivityLevel::Error)
            .collect()
    }

    /// Get error messages from Activity::Message variants.
    /// This catches errors that may have been created as activities but not in message_log.
    pub fn get_activity_error_messages(&self) -> Vec<(&str, Option<&str>)> {
        self.activities
            .values()
            .filter_map(|activity| {
                if let ActivityVariant::Message(msg_data) = &activity.variant
                    && msg_data.level == ActivityLevel::Error
                {
                    Some((activity.name.as_str(), msg_data.details.as_deref()))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get stderr output from failed or incomplete builds.
    /// Returns tuples of (build_name, stderr_lines).
    ///
    /// This includes:
    /// - Builds that completed with success=false
    /// - Builds that are still active/queued but have stderr output
    ///   (the FFI layer doesn't always report failures properly)
    pub fn get_failed_build_errors(&self) -> Vec<(&str, &[String])> {
        self.activities
            .values()
            .filter_map(|activity| {
                if let ActivityVariant::Build(build) = &activity.variant
                    && !build.log_stderr_lines.is_empty()
                {
                    // Include if explicitly failed OR still active (likely unreported failure)
                    let is_failed = matches!(
                        activity.state,
                        NixActivityState::Completed { success: false, .. }
                    );
                    let is_incomplete = matches!(
                        activity.state,
                        NixActivityState::Active | NixActivityState::Queued
                    );
                    if is_failed || is_incomplete {
                        return Some((activity.name.as_str(), build.log_stderr_lines.as_slice()));
                    }
                }
                None
            })
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

    /// Get children of an activity without level filtering.
    /// Used for traversing through filtered parents to find visible descendants.
    /// Prioritizes active activities, then lingering completed ones, then older completed ones.
    /// Returns a tuple of (visible_children, total_children_count, hidden_count).
    ///
    /// Task activities always show all their children without linger/limit restrictions,
    /// so completed tasks remain visible after execution.
    fn get_children(
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

        // Get all children of this parent (including additional parents),
        // excluding hidden and promoted-to-top-level activities.
        let mut all_children: Vec<_> = self
            .activities
            .values()
            .filter(|a| {
                if matches!(a.variant, ActivityVariant::UserOperation)
                    || a.variant.is_always_top_level()
                {
                    return false;
                }
                // Check primary parent
                if a.parent_id == Some(parent_id) {
                    return true;
                }
                // Check additional parents
                self.additional_parents
                    .get(&a.id)
                    .is_some_and(|parents| parents.contains(&parent_id))
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
        let (active, completed): (Vec<_>, Vec<_>) = all_children
            .into_iter()
            .partition(|a| matches!(a.state, NixActivityState::Queued | NixActivityState::Active));

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
                    && !a.variant.is_always_top_level()
                    && a.level <= filter_level
            })
            .count()
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
    /// Expected build count from Nix (via SetExpected events)
    pub expected_builds: Option<u64>,
    pub active_downloads: usize,
    pub completed_downloads: usize,
    /// Expected download count from Nix (via SetExpected events)
    pub expected_downloads: Option<u64>,
    pub active_queries: usize,
    pub completed_queries: usize,
    pub running_tasks: usize,
    pub completed_tasks: usize,
    pub failed_tasks: usize,
    pub running_processes: usize,
}

/// Format an EvalOp as a display string for logging.
fn format_eval_op(op: &EvalOp) -> String {
    match op {
        EvalOp::CopiedSource { source, target } => {
            format!(
                "copied source '{}' -> '{}'",
                source.display(),
                target.display()
            )
        }
        EvalOp::EvaluatedFile { source } => format!("evaluating file '{}'", source.display()),
        EvalOp::ReadFile { source } => format!("devenv readFile: '{}'", source.display()),
        EvalOp::ReadDir { source } => format!("devenv readDir: '{}'", source.display()),
        EvalOp::GetEnv { name } => format!("devenv getEnv: '{}'", name),
        EvalOp::PathExists { source } => format!("devenv pathExists: '{}'", source.display()),
        EvalOp::TrackedPath { source } => format!("trace: devenv path: '{}'", source.display()),
    }
}
