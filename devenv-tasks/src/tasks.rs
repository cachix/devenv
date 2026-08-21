use crate::config::{Config, RunMode, parse_dependency};
use crate::error::Error;
use crate::task_cache::TaskCache;
use crate::task_state::TaskState;
use crate::types::{
    DepSatisfaction, DependencyKind, OneshotStatus, Output, Outputs, PROCESS_TASK_PREFIX, Skipped,
    TaskCompleted, TaskFailure, TaskStatus, TaskType, TasksStatus, VerbosityLevel,
};
use devenv_activity::{Activity, ActivityInstrument, TaskInfo, emit_task_hierarchy, next_id};
use devenv_processes::{
    ExitStatus, NativeProcessManager, ProcessConfig, ProcessPhase, StartOutcome, SupervisionMode,
};
use petgraph::algo::{has_path_connecting, toposort};
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::{EdgeRef, Reversed};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, Notify, RwLock};
use tokio::time::Instant;
use tracing::{error, instrument};

/// Builder for Tasks configuration
pub struct TasksBuilder {
    config: Config,
    verbosity: VerbosityLevel,
    db_path: Option<PathBuf>,
    shutdown: Arc<tokio_shutdown::Shutdown>,
    refresh_task_cache: bool,
}

impl TasksBuilder {
    /// Create a new builder with required configuration
    pub fn new(
        config: Config,
        verbosity: VerbosityLevel,
        shutdown: Arc<tokio_shutdown::Shutdown>,
    ) -> Self {
        Self {
            config,
            verbosity,
            db_path: None,
            shutdown,
            refresh_task_cache: false,
        }
    }

    /// Set the database path for task caching
    pub fn with_db_path(mut self, db_path: PathBuf) -> Self {
        self.db_path = Some(db_path);
        self
    }

    /// Force a refresh of the task cache, skipping cache reads
    pub fn with_refresh_task_cache(mut self, refresh_task_cache: bool) -> Self {
        self.refresh_task_cache = refresh_task_cache;
        self
    }

    /// Build the Tasks instance
    pub async fn build(self) -> Result<Tasks, Error> {
        let supervisor = self.config.supervisor;
        // External managers own ordering and invoke one wrapper per process.
        let ignore_process_deps =
            self.config.ignore_process_deps || supervisor == SupervisionMode::External;
        let exit_on_idle = self
            .config
            .exit_on_idle
            .unwrap_or(supervisor == SupervisionMode::External);

        let cache = if let Some(db_path) = self.db_path {
            TaskCache::with_db_path(db_path)
                .await
                .map_err(|e| Error::io(format!("Failed to initialize task cache: {e}")))?
        } else {
            TaskCache::new(&self.config.cache_dir)
                .await
                .map_err(|e| Error::io(format!("Failed to initialize task cache: {e}")))?
        };

        // Create process manager for long-running process tasks
        let mut pm = NativeProcessManager::new(self.config.runtime_dir.clone())
            .map_err(|e| Error::io(format!("Failed to initialize process manager: {e}")))?;
        if supervisor == SupervisionMode::External {
            pm.disown_runtime_files();
        }

        let notify_finished = Arc::new(Notify::new());
        pm.set_task_notify(Arc::clone(&notify_finished));
        let process_manager = Arc::new(pm);

        let mut graph = DiGraph::new();
        let mut task_indices = HashMap::new();
        for task in self.config.tasks {
            let name = task.name.clone();
            if !task.name.contains(':')
                || task.name.split(':').count() < 2
                || task.name.starts_with(':')
                || task.name.ends_with(':')
                || task.name.contains('@')
                || !task
                    .name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == ':' || c == '_' || c == '-')
            {
                return Err(Error::InvalidTaskName(name));
            }
            if task.status.is_some() && task.command.is_none() {
                return Err(Error::MissingCommand(name));
            }
            let index = graph.add_node(Arc::new(RwLock::new(TaskState::new(
                task,
                self.verbosity,
                self.config.sudo_context.clone(),
            ))));
            task_indices.insert(name, index);
        }

        let roots = Tasks::resolve_namespace_roots(&self.config.roots, &task_indices)?;
        let mut tasks = Tasks {
            roots,
            root_names: self.config.roots,
            graph,
            notify_finished,
            notify_ui: Arc::new(Notify::new()),
            tasks_order: vec![],
            run_mode: self.config.run_mode,
            cache,
            shutdown: self.shutdown,
            process_manager,
            env: self.config.env,
            bash: self.config.bash,
            refresh_task_cache: self.refresh_task_cache,
            ignore_process_deps,
            task_index_by_name: HashMap::new(),
            start_with_deps_lock: Mutex::new(()),
            scheduled_task_indices: Mutex::new(HashSet::new()),
            outputs: Arc::new(Mutex::new(Outputs::new())),
            exit_on_idle,
            supervisor,
        };

        tasks.resolve_dependencies(task_indices).await?;
        tasks.tasks_order = tasks.schedule().await?;
        tasks.scheduled_task_indices = Mutex::new(tasks.tasks_order.iter().copied().collect());
        // Dynamic starts address nodes outside the initial schedule.
        for index in tasks.graph.node_indices() {
            let name = tasks.graph[index].read().await.task.name.clone();
            tasks.task_index_by_name.insert(name, index);
        }
        Ok(tasks)
    }
}

impl std::fmt::Debug for Tasks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tasks")
            .field("root_names", &self.root_names)
            .field("run_mode", &self.run_mode)
            .field("shutdown", &"<Shutdown>")
            .finish()
    }
}

pub struct Tasks {
    pub(crate) roots: Vec<NodeIndex>,
    // Stored for reporting
    pub(crate) root_names: Vec<String>,
    pub(crate) graph: DiGraph<Arc<RwLock<TaskState>>, DependencyKind>,
    pub(crate) tasks_order: Vec<NodeIndex>,
    pub(crate) notify_finished: Arc<Notify>,
    pub(crate) notify_ui: Arc<Notify>,
    pub(crate) run_mode: RunMode,
    pub(crate) cache: TaskCache,
    pub(crate) shutdown: Arc<tokio_shutdown::Shutdown>,
    /// Process manager for running long-lived process tasks
    pub(crate) process_manager: Arc<NativeProcessManager>,
    /// Environment variables to pass to processes
    pub(crate) env: HashMap<String, String>,
    /// Path to the bash binary to use for probe commands
    pub(crate) bash: String,
    /// Force a refresh of the task cache, skipping cache reads
    pub(crate) refresh_task_cache: bool,
    /// When true, exclude non-root process-type tasks from the scheduled subgraph
    pub(crate) ignore_process_deps: bool,
    /// Full task name to full-graph node index.
    pub(crate) task_index_by_name: HashMap<String, NodeIndex>,
    /// Prevents concurrent dynamic starts from duplicating dependency waiters.
    pub(crate) start_with_deps_lock: Mutex<()>,
    /// Nodes with an execution driver; `Pending` alone does not distinguish
    /// scheduled from unscheduled one-shots.
    pub(crate) scheduled_task_indices: Mutex<HashSet<NodeIndex>>,
    /// Shared outputs from cold and dynamic one-shot runs.
    pub(crate) outputs: Arc<Mutex<Outputs>>,
    /// Exit after every process has settled when an outer manager owns lifecycle.
    pub(crate) exit_on_idle: bool,
    /// Selects native or external lifecycle policy for registered processes.
    pub(crate) supervisor: devenv_processes::SupervisionMode,
}

/// Shared dependency evaluation for waiting and parked-state checks.
struct DepEval {
    /// Full task name of the dependency (e.g. `devenv:processes:db`).
    task_name: String,
    sat: DepSatisfaction,
    /// Live phase for a registered process dependency.
    live_phase: Option<ProcessPhase>,
    /// Whether a one-shot dependency is currently executing.
    dep_in_flight: bool,
}

/// Dependency wait result; cancellation takes precedence over failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DependencyWaitOutcome {
    Ready,
    Cancelled,
    DependencyFailed,
}

impl Tasks {
    /// Create a new TasksBuilder for configuring Tasks
    pub fn builder(
        config: Config,
        verbosity: VerbosityLevel,
        shutdown: std::sync::Arc<tokio_shutdown::Shutdown>,
    ) -> TasksBuilder {
        TasksBuilder::new(config, verbosity, shutdown)
    }

    /// Returns a reference to the process manager used for long-lived process tasks.
    pub fn process_manager(&self) -> &Arc<NativeProcessManager> {
        &self.process_manager
    }

    /// Get the current task completion status
    pub async fn get_completion_status(&self) -> TasksStatus {
        let mut status = TasksStatus::new();

        // `tasks_order` is fixed after `schedule()`, so the scheduled set is
        // constant for this call; build it once rather than per failed task.
        let scheduled: HashSet<NodeIndex> = self.tasks_order.iter().copied().collect();

        for index in &self.tasks_order {
            let task_state = self.graph[*index].read().await;
            match &task_state.status {
                // A process task node stays `Pending` for its whole live
                // lifecycle; the manager owns the phase. A `Completed` node
                // always wins below: it is the graph-owned launch outcome.
                TaskStatus::Pending if task_state.task.r#type == TaskType::Process => {
                    let pname = crate::types::process_name(&task_state.task.name);
                    match self.process_manager.get_phase(pname).await {
                        Some(ProcessPhase::NotStarted | ProcessPhase::Stopped) => {
                            status.skipped += 1
                        }
                        Some(ProcessPhase::Exited) => {
                            if self.process_manager.get_exit_status(pname).await
                                == Some(ExitStatus::Failure)
                            {
                                status.failed += 1;
                                if self.is_soft_failure(index, &scheduled) {
                                    status.soft_failed += 1;
                                }
                            } else {
                                status.succeeded += 1;
                            }
                        }
                        Some(ProcessPhase::GaveUp) => {
                            status.failed += 1;
                            if self.is_soft_failure(index, &scheduled) {
                                status.soft_failed += 1;
                            }
                        }
                        Some(
                            ProcessPhase::Waiting
                            | ProcessPhase::Starting
                            | ProcessPhase::Ready
                            | ProcessPhase::Stopping,
                        ) => status.running += 1,
                        None => status.pending += 1,
                    }
                }
                TaskStatus::Pending => status.pending += 1,
                TaskStatus::Oneshot(OneshotStatus::Running(_)) => status.running += 1,
                TaskStatus::Completed(completed) => match completed {
                    TaskCompleted::Success(_, _) => status.succeeded += 1,
                    TaskCompleted::Failed(_, _) => {
                        status.failed += 1;
                        if self.is_soft_failure(index, &scheduled) {
                            status.soft_failed += 1;
                        }
                    }
                    TaskCompleted::Skipped(_) => status.skipped += 1,
                    TaskCompleted::DependencyFailed => {
                        status.dependency_failed += 1;
                        if self.is_soft_failure(index, &scheduled) {
                            status.soft_dependency_failed += 1;
                        }
                    }
                    TaskCompleted::Cancelled(_) => status.cancelled += 1,
                },
            }
        }

        status
    }

    /// Check if a failed task at `index` is a "soft" failure.
    ///
    /// A failure is soft if:
    /// 1. The task is NOT a root task, AND
    /// 2. The task has at least one outgoing edge (someone depends on it), AND
    /// 3. ALL outgoing edges use `DependencyKind::Completed`
    fn is_soft_failure(&self, index: &NodeIndex, scheduled: &HashSet<NodeIndex>) -> bool {
        if self.roots.contains(index) {
            return false;
        }
        // Only dependents scheduled in this run count. The graph now retains
        // tasks that were not scheduled (so they stay startable later), and an
        // unscheduled dependent must not change how a failure is classified.
        let outgoing: Vec<_> = self
            .graph
            .edges_directed(*index, petgraph::Direction::Outgoing)
            .filter(|e| scheduled.contains(&e.target()))
            .collect();
        !outgoing.is_empty()
            && outgoing
                .iter()
                .all(|e| *e.weight() == DependencyKind::Completed)
    }

    fn resolve_namespace_roots(
        roots: &[String],
        task_indices: &HashMap<String, NodeIndex>,
    ) -> Result<Vec<NodeIndex>, Error> {
        let mut resolved_roots = Vec::new();

        for name in roots {
            let trimmed_name = name.trim();

            // Validate namespace name
            if trimmed_name.is_empty() {
                return Err(Error::TaskNotFound(name.clone()));
            }

            // Reject invalid namespace patterns
            if trimmed_name == ":" || trimmed_name.starts_with(':') || trimmed_name.contains("::") {
                return Err(Error::TaskNotFound(name.clone()));
            }

            // Check for exact match first
            if let Some(index) = task_indices.get(trimmed_name) {
                resolved_roots.push(*index);
                continue;
            }

            // Check if this is a namespace prefix (with or without colon)
            let search_prefix: Cow<str> = if trimmed_name.ends_with(':') {
                Cow::Borrowed(trimmed_name)
            } else {
                Cow::Owned(format!("{trimmed_name}:"))
            };

            // Find all tasks with this prefix
            let matching_tasks: Vec<_> = task_indices
                .iter()
                .filter(|(task_name, _)| task_name.starts_with(&*search_prefix))
                .map(|(_, &index)| index)
                .collect();

            if !matching_tasks.is_empty() {
                resolved_roots.extend(matching_tasks);
                continue;
            }

            return Err(Error::TaskNotFound(name.clone()));
        }

        Ok(resolved_roots)
    }

    async fn resolve_dependencies(
        &mut self,
        task_indices: HashMap<String, NodeIndex>,
    ) -> Result<(), Error> {
        let mut unresolved = HashSet::new();
        let mut edges_to_add = Vec::new();
        let mut validation_errors = Vec::new();

        for index in self.graph.node_indices() {
            let task_state = &self.graph[index].read().await;

            for dep_name in &task_state.task.after {
                // Parse dependency with optional suffix
                let dep_spec = parse_dependency(dep_name)?;

                if let Some(dep_idx) = task_indices.get(&dep_spec.name) {
                    let dep_task = &self.graph[*dep_idx].read().await;

                    // Resolve the dependency kind based on task type if not explicitly specified
                    // Default: Ready for process tasks, Succeeded for oneshot tasks
                    let resolved_kind = dep_spec.kind.unwrap_or_else(|| {
                        if dep_task.task.r#type == TaskType::Process {
                            DependencyKind::Ready
                        } else {
                            DependencyKind::Succeeded
                        }
                    });

                    // Validate suffix is compatible with the dependency's task type
                    match (dep_task.task.r#type, resolved_kind) {
                        (TaskType::Oneshot, DependencyKind::Ready) => {
                            validation_errors.push(format!(
                                "Task '{}' depends on '{}@ready' but '{}' is a oneshot task. \
                                 Oneshot tasks support @started, @succeeded, and @completed suffixes.",
                                task_state.task.name, dep_spec.name, dep_spec.name
                            ));
                        }
                        (TaskType::Process, DependencyKind::Succeeded) => {
                            validation_errors.push(format!(
                                "Task '{}' depends on '{}@succeeded' but '{}' is a process task. \
                                 Process tasks support @started, @ready, and @completed suffixes.",
                                task_state.task.name, dep_spec.name, dep_spec.name
                            ));
                        }
                        _ => {}
                    }

                    // Validate @ready dependencies on process tasks require ready or listen
                    if resolved_kind == DependencyKind::Ready
                        && dep_task.task.r#type == TaskType::Process
                        && !process_has_ready_config(&dep_task.task)
                    {
                        validation_errors.push(format!(
                            "Task '{}' depends on '{}@ready' but process has no ready config, TCP listen config, or allocated ports. \
                             Add a ready probe, configure a TCP listen socket, or allocate ports for the process. \
                             See https://devenv.sh/processes/#ready-probes",
                            task_state.task.name, dep_spec.name
                        ));
                    }
                    edges_to_add.push((*dep_idx, index, resolved_kind));
                } else {
                    unresolved.insert((task_state.task.name.clone(), dep_name.clone()));
                }
            }

            for before_name in &task_state.task.before {
                // Parse dependency with optional suffix
                let dep_spec = parse_dependency(before_name)?;

                if let Some(before_idx) = task_indices.get(&dep_spec.name) {
                    // For 'before' relationships, the current task is the dependency source
                    // Resolve kind based on current task's type if not explicitly specified
                    let resolved_kind = dep_spec.kind.unwrap_or_else(|| {
                        if task_state.task.r#type == TaskType::Process {
                            DependencyKind::Ready
                        } else {
                            DependencyKind::Succeeded
                        }
                    });

                    // Validate suffix is compatible with the current task's type
                    match (task_state.task.r#type, resolved_kind) {
                        (TaskType::Oneshot, DependencyKind::Ready) => {
                            validation_errors.push(format!(
                                "Task '{}' declares before '{}' with @ready but '{}' is a oneshot task. \
                                 Oneshot tasks support @started, @succeeded, and @completed suffixes.",
                                task_state.task.name, dep_spec.name, task_state.task.name
                            ));
                        }
                        (TaskType::Process, DependencyKind::Succeeded) => {
                            validation_errors.push(format!(
                                "Task '{}' declares before '{}' with @succeeded but '{}' is a process task. \
                                 Process tasks support @started, @ready, and @completed suffixes.",
                                task_state.task.name, dep_spec.name, task_state.task.name
                            ));
                        }
                        _ => {}
                    }

                    // Validate @ready dependencies - current task must have ready or listen if it's a process
                    if resolved_kind == DependencyKind::Ready
                        && task_state.task.r#type == TaskType::Process
                        && !process_has_ready_config(&task_state.task)
                    {
                        validation_errors.push(format!(
                            "Process '{}' has tasks depending on it via @ready but has no ready config, TCP listen config, or allocated ports. \
                             Add a ready probe, configure a TCP listen socket, or allocate ports for the process. \
                             See https://devenv.sh/processes/#ready-probes",
                            task_state.task.name
                        ));
                    }
                    edges_to_add.push((index, *before_idx, resolved_kind));
                } else {
                    unresolved.insert((task_state.task.name.clone(), before_name.clone()));
                }
            }
        }

        // Return validation errors first
        if !validation_errors.is_empty() {
            return Err(Error::InvalidDependency(validation_errors.join("\n")));
        }

        for (from, to, kind) in edges_to_add {
            self.graph.update_edge(from, to, kind);
        }

        if unresolved.is_empty() {
            Ok(())
        } else {
            Err(Error::TasksNotFound(unresolved.into_iter().collect()))
        }
    }

    #[instrument(skip(self), fields(graph, subgraph), ret)]
    async fn schedule(&mut self) -> Result<Vec<NodeIndex>, Error> {
        let mut subgraph = DiGraph::new();
        let mut node_map = HashMap::new();
        let mut visited = HashSet::new();
        let mut to_visit = Vec::new();

        // Start with root nodes
        for &root_index in &self.roots {
            to_visit.push(root_index);
        }

        // Find nodes to include based on run_mode
        match self.run_mode {
            RunMode::Single => {
                // Only include the root nodes themselves
                visited = self.roots.iter().cloned().collect();
            }
            RunMode::After => {
                // Include root nodes and all tasks that come after (successor nodes)
                while let Some(node) = to_visit.pop() {
                    if visited.insert(node) {
                        // Add outgoing neighbors (tasks that come after this one)
                        for neighbor in self
                            .graph
                            .neighbors_directed(node, petgraph::Direction::Outgoing)
                        {
                            to_visit.push(neighbor);
                        }
                    }
                }
            }
            RunMode::Before => {
                // Include root nodes and all tasks that come before (predecessor nodes)
                while let Some(node) = to_visit.pop() {
                    if visited.insert(node) {
                        // Add incoming neighbors (tasks that come before this one)
                        for neighbor in self
                            .graph
                            .neighbors_directed(node, petgraph::Direction::Incoming)
                        {
                            to_visit.push(neighbor);
                        }
                    }
                }
            }
            RunMode::All => {
                // Include prerequisites (incoming) and dependents (outgoing) separately.
                // This avoids "direction bouncing" through intermediate nodes that would
                // incorrectly include unrelated tasks sharing a common prerequisite.
                // See: https://github.com/cachix/devenv/issues/2337

                // First: traverse incoming edges (prerequisites) from roots
                while let Some(node) = to_visit.pop() {
                    if visited.insert(node) {
                        for neighbor in self
                            .graph
                            .neighbors_directed(node, petgraph::Direction::Incoming)
                        {
                            to_visit.push(neighbor);
                        }
                    }
                }

                // Second: traverse outgoing edges (dependents) from roots
                // Start by adding outgoing neighbors of roots (roots are already visited)
                for &root_index in &self.roots {
                    for neighbor in self
                        .graph
                        .neighbors_directed(root_index, petgraph::Direction::Outgoing)
                    {
                        to_visit.push(neighbor);
                    }
                }
                while let Some(node) = to_visit.pop() {
                    if visited.insert(node) {
                        for neighbor in self
                            .graph
                            .neighbors_directed(node, petgraph::Direction::Outgoing)
                        {
                            to_visit.push(neighbor);
                        }
                    }
                }

                // Include every selected task's prerequisites without traversing
                // outward from those prerequisites.
                to_visit.extend(visited.iter().copied());
                while let Some(node) = to_visit.pop() {
                    for neighbor in self
                        .graph
                        .neighbors_directed(node, petgraph::Direction::Incoming)
                    {
                        if visited.insert(neighbor) {
                            to_visit.push(neighbor);
                        }
                    }
                }
            }
        }

        // External managers own ordering for non-root process tasks.
        if self.ignore_process_deps {
            let root_set: HashSet<NodeIndex> = self.roots.iter().cloned().collect();
            let mut to_remove = Vec::new();
            for &node in &visited {
                if root_set.contains(&node) {
                    continue;
                }
                let task_state = self.graph[node].read().await;
                if task_state.task.r#type == TaskType::Process {
                    to_remove.push(node);
                }
            }
            for node in to_remove {
                visited.remove(&node);
            }
        }

        // Create nodes in the subgraph
        for &node in &visited {
            let new_node = subgraph.add_node(self.graph[node].clone());
            node_map.insert(node, new_node);
        }

        // Add edges to subgraph, preserving edge weights
        for (&old_node, &new_node) in &node_map {
            for edge in self.graph.edges(old_node) {
                let target = edge.target();
                if let Some(&new_target) = node_map.get(&target) {
                    subgraph.add_edge(new_node, new_target, *edge.weight());
                }
            }
        }

        // Retain the full graph for dynamic starts; the subgraph only determines
        // the initial order.
        let full_by_sub: HashMap<NodeIndex, NodeIndex> =
            node_map.iter().map(|(&full, &sub)| (sub, full)).collect();

        // Map the scheduled order back to full-graph indices.
        match toposort(&subgraph, None) {
            Ok(order) => Ok(order.into_iter().map(|sub| full_by_sub[&sub]).collect()),
            Err(cycle) => Err(Error::CycleDetected(
                subgraph[cycle.node_id()].read().await.task.name.clone(),
            )),
        }
    }

    #[instrument(skip(self))]
    pub async fn run(&self, is_process_mode: bool) -> Outputs {
        let (label, item_type) = if is_process_mode {
            ("Running processes", "processes")
        } else {
            ("Running tasks", "tasks")
        };
        let orchestration_activity = Arc::new(devenv_activity::start!(
            Activity::operation(label).parent(None).detail(format!(
                "{} {}, roots: {:?}",
                self.tasks_order.len(),
                item_type,
                self.root_names
            ))
        ));

        self.run_internal(orchestration_activity, is_process_mode)
            .await
    }

    /// Run process tasks under a caller-provided activity.
    #[instrument(skip(self, parent_activity))]
    pub async fn run_with_parent_activity(&self, parent_activity: Arc<Activity>) -> Outputs {
        self.run_internal(parent_activity, true).await
    }

    /// Schedule named process tasks and their dependencies against the live graph.
    ///
    /// Names omit the `devenv:processes:` prefix. The result classifies each
    /// unique name as scheduled, skipped, unknown, or failed. This returns after
    /// scheduling; dependencies may keep a process `Waiting` in the background.
    pub async fn start_with_deps<I, S>(&self, names: I) -> StartOutcome
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        // Prevent concurrent calls from spawning duplicate dependency waiters.
        let _serialize = self.start_with_deps_lock.lock().await;
        let mut outcome = StartOutcome::default();

        // Deduplicate without changing result order.
        let mut seen = std::collections::HashSet::new();
        for name in names.into_iter().map(Into::into) {
            if !seen.insert(name.clone()) {
                continue;
            }
            let task_name = format!("{PROCESS_TASK_PREFIX}{name}");
            let Some(&index) = self.task_index_by_name.get(&task_name) else {
                tracing::debug!(process = %name, "up requested for a process not in the task graph");
                outcome.unknown.push(name.clone());
                continue;
            };

            match self.process_manager.get_phase(&name).await {
                // Preserve the existing driver for active and scheduled processes.
                Some(ProcessPhase::Starting | ProcessPhase::Ready | ProcessPhase::Waiting) => {
                    outcome.skipped.push(name.clone());
                    continue;
                }
                // `rearm_waiting` only replaces inactive manager entries.
                Some(ProcessPhase::Exited | ProcessPhase::GaveUp) => {
                    if let Err(e) = self.process_manager.stop_and_keep(&name).await {
                        tracing::warn!(
                            process = %name,
                            error = %e,
                            "failed to reset exited process before relaunch"
                        );
                    }
                }
                _ => {}
            }

            // Explicit selection overrides `start.enable`.
            let config = {
                let ts = self.graph[index].read().await;
                match ts.build_process_config(&self.env, &self.bash, self.supervisor) {
                    Ok(mut config) => {
                        config.start.enable = true;
                        config
                    }
                    Err(e) => {
                        tracing::error!(
                            process = %name,
                            error = %e,
                            "failed to build process config"
                        );
                        outcome.failed.push(name.clone());
                        continue;
                    }
                }
            };
            // Cold subsets leave unseen one-shot predecessors without a driver.
            self.schedule_unseen_oneshot_dependencies(index).await;
            self.process_manager.rearm_waiting(config.clone()).await;
            // Make the manager's re-armed phase authoritative again.
            {
                let mut ts = self.graph[index].write().await;
                ts.status = TaskStatus::Pending;
            }
            self.notify_finished.notify_waiters();

            outcome.scheduled.push(name.clone());

            let deps = self.collect_deps(index);
            let task_state = Arc::clone(&self.graph[index]);
            let notify_finished = Arc::clone(&self.notify_finished);
            let process_manager = Arc::clone(&self.process_manager);
            let shutdown = Arc::clone(&self.shutdown);
            let process_name = name.clone();

            // Detached waiters keep unsatisfied starts visible without blocking replies.
            tokio::spawn(async move {
                match Self::wait_for_task_deps(&deps, &process_manager, &notify_finished, &shutdown)
                    .await
                {
                    DependencyWaitOutcome::Ready => {}
                    outcome => {
                        process_manager.cancel_waiting(&process_name).await;
                        // Publish the graph outcome before waking transitive waiters.
                        task_state.write().await.status = TaskStatus::Completed(match outcome {
                            DependencyWaitOutcome::Cancelled => TaskCompleted::Cancelled(None),
                            DependencyWaitOutcome::DependencyFailed => {
                                TaskCompleted::DependencyFailed
                            }
                            DependencyWaitOutcome::Ready => unreachable!(),
                        });
                        notify_finished.notify_waiters();
                        return;
                    }
                }

                // Scope the read guard before the failure-path write lock.
                let launch_result = {
                    let ts = task_state.read().await;
                    ts.run_process(&process_manager, config).await
                };
                if let Err(e) = launch_result {
                    tracing::error!(
                        process = %process_name,
                        error = %e,
                        "failed to start process"
                    );
                    // Dependents read launch failures from the graph.
                    let mut ts = task_state.write().await;
                    ts.status = TaskStatus::Completed(TaskCompleted::Failed(
                        std::time::Duration::ZERO,
                        TaskFailure {
                            stdout: Vec::new(),
                            stderr: Vec::new(),
                            error: format!("Failed to start process: {e}"),
                        },
                    ));
                    drop(ts);
                    notify_finished.notify_waiters();
                }
            });
        }

        outcome
    }

    /// Publish one task's terminal progress and wake observers.
    fn signal_task_done(
        completed_tasks: &std::sync::atomic::AtomicU64,
        total_tasks: u64,
        orchestration_activity: &Activity,
        notify_finished: &Notify,
        notify_ui: &Notify,
    ) {
        let done = completed_tasks.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        orchestration_activity.progress(done, total_tasks, None);
        notify_finished.notify_waiters();
        notify_ui.notify_one();
    }

    /// Publish a cancelled or dependency-failed task.
    async fn mark_task_skipped(
        task_state: &Arc<RwLock<TaskState>>,
        task_activity_id: u64,
        cancelled: bool,
        completed_tasks: &std::sync::atomic::AtomicU64,
        total_tasks: u64,
        orchestration_activity: &Activity,
        notify_finished: &Notify,
        notify_ui: &Notify,
    ) {
        let task_completed = if cancelled {
            TaskCompleted::Cancelled(None)
        } else {
            TaskCompleted::DependencyFailed
        };

        let task_name = task_state.read().await.task.name.clone();
        let skip_activity =
            devenv_activity::start!(Activity::task(&task_name).id(task_activity_id));
        if cancelled {
            skip_activity.cancel();
        } else {
            skip_activity.dependency_failed();
        }

        {
            let mut ts = task_state.write().await;
            ts.status = TaskStatus::Completed(task_completed);
        }

        Self::signal_task_done(
            completed_tasks,
            total_tasks,
            orchestration_activity,
            notify_finished,
            notify_ui,
        );
    }

    /// Collect dependency edges for a task node.
    fn collect_deps(&self, index: NodeIndex) -> Vec<(Arc<RwLock<TaskState>>, DependencyKind)> {
        self.graph
            .edges_directed(index, petgraph::Direction::Incoming)
            .map(|edge| (self.graph[edge.source()].clone(), *edge.weight()))
            .collect()
    }

    /// Dependencies inside the cold schedule.
    fn collect_scheduled_deps(
        &self,
        index: NodeIndex,
        scheduled: &HashSet<NodeIndex>,
    ) -> Vec<(Arc<RwLock<TaskState>>, DependencyKind)> {
        self.graph
            .edges_directed(index, petgraph::Direction::Incoming)
            .filter(|edge| scheduled.contains(&edge.source()))
            .map(|edge| (self.graph[edge.source()].clone(), *edge.weight()))
            .collect()
    }

    /// Start unseen one-shots in a dynamic dependency closure exactly once.
    /// Process dependencies still require explicit starts.
    async fn schedule_unseen_oneshot_dependencies(&self, index: NodeIndex) {
        let mut stack = vec![index];
        let mut visited = HashSet::new();
        let mut oneshots = Vec::new();

        while let Some(node) = stack.pop() {
            if !visited.insert(node) {
                continue;
            }
            for edge in self
                .graph
                .edges_directed(node, petgraph::Direction::Incoming)
            {
                stack.push(edge.source());
            }
            if node != index && self.graph[node].read().await.task.r#type == TaskType::Oneshot {
                oneshots.push(node);
            }
        }

        let to_schedule = {
            let mut scheduled = self.scheduled_task_indices.lock().await;
            oneshots
                .into_iter()
                .filter(|node| scheduled.insert(*node))
                .collect::<Vec<_>>()
        };

        for node in to_schedule {
            let task_state = Arc::clone(&self.graph[node]);
            if !matches!(task_state.read().await.status, TaskStatus::Pending) {
                continue;
            }

            let deps = self.collect_deps(node);
            let outputs = Arc::clone(&self.outputs);
            let notify_finished = Arc::clone(&self.notify_finished);
            let notify_ui = Arc::clone(&self.notify_ui);
            let cache = Arc::new(self.cache.clone());
            let shutdown = Arc::clone(&self.shutdown);
            let process_manager = Arc::clone(&self.process_manager);
            let refresh_task_cache = self.refresh_task_cache;
            let shell_env = self.env.clone();
            let task_activity_id = next_id();

            tokio::spawn(async move {
                Self::run_oneshot_task(
                    task_state,
                    deps,
                    outputs,
                    notify_finished,
                    notify_ui,
                    cache,
                    shutdown,
                    process_manager,
                    task_activity_id,
                    refresh_task_cache,
                    shell_env,
                )
                .await;
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_oneshot_task(
        task_state: Arc<RwLock<TaskState>>,
        deps: Vec<(Arc<RwLock<TaskState>>, DependencyKind)>,
        outputs: Arc<Mutex<Outputs>>,
        notify_finished: Arc<Notify>,
        notify_ui: Arc<Notify>,
        cache: Arc<TaskCache>,
        shutdown: Arc<tokio_shutdown::Shutdown>,
        process_manager: Arc<NativeProcessManager>,
        task_activity_id: u64,
        refresh_task_cache: bool,
        shell_env: HashMap<String, String>,
    ) {
        match Self::wait_for_task_deps(&deps, &process_manager, &notify_finished, &shutdown).await {
            DependencyWaitOutcome::Ready => {}
            outcome => {
                let task_name = task_state.read().await.task.name.clone();
                let task_activity =
                    devenv_activity::start!(Activity::task(&task_name).id(task_activity_id));
                let completed = match outcome {
                    DependencyWaitOutcome::Cancelled => {
                        task_activity.cancel();
                        TaskCompleted::Cancelled(None)
                    }
                    DependencyWaitOutcome::DependencyFailed => {
                        task_activity.dependency_failed();
                        TaskCompleted::DependencyFailed
                    }
                    DependencyWaitOutcome::Ready => unreachable!(),
                };
                task_state.write().await.status = TaskStatus::Completed(completed);
                notify_finished.notify_waiters();
                notify_ui.notify_one();
                return;
            }
        }

        let now = Instant::now();
        task_state.write().await.status = TaskStatus::Oneshot(OneshotStatus::Running(now));
        notify_ui.notify_one();

        let completed = {
            let outputs = outputs.lock().await.clone();
            match task_state
                .read()
                .await
                .run(
                    now,
                    &outputs,
                    &cache,
                    shutdown.cancellation_token(),
                    task_activity_id,
                    refresh_task_cache,
                    &shell_env,
                )
                .await
            {
                Ok(result) => result,
                Err(e) => {
                    error!(error = %e, "task failed");
                    TaskCompleted::Failed(
                        now.elapsed(),
                        TaskFailure {
                            stdout: Vec::new(),
                            stderr: Vec::new(),
                            error: format!("Task failed: {e:#}"),
                        },
                    )
                }
            }
        };

        {
            let mut task_state = task_state.write().await;
            match &completed {
                TaskCompleted::Success(_, Output(Some(output))) => {
                    outputs
                        .lock()
                        .await
                        .insert(task_state.task.name.clone(), output.clone());

                    if let Some(output_value) = output.as_object() {
                        let task_name = &task_state.task.name;
                        if let Err(e) = cache
                            .store_task_output(
                                task_name,
                                &serde_json::Value::Object(output_value.clone()),
                            )
                            .await
                        {
                            tracing::warn!(
                                task = %task_name,
                                error = %e,
                                "failed to store task output"
                            );
                        }
                    }
                }
                TaskCompleted::Skipped(Skipped::Cached(Output(Some(output)))) => {
                    outputs
                        .lock()
                        .await
                        .insert(task_state.task.name.clone(), output.clone());

                    if (task_state.task.status.is_some()
                        || !task_state.task.exec_if_modified.is_empty())
                        && let Some(output_value) = output.as_object()
                    {
                        let task_name = &task_state.task.name;
                        if let Err(e) = cache
                            .store_task_output(
                                task_name,
                                &serde_json::Value::Object(output_value.clone()),
                            )
                            .await
                        {
                            tracing::warn!(
                                task = %task_name,
                                error = %e,
                                "failed to store task output"
                            );
                        }
                    }
                }
                _ => {}
            }

            task_state.status = TaskStatus::Completed(completed);
        }
        notify_finished.notify_waiters();
        notify_ui.notify_one();
    }

    /// Evaluate an edge consistently for waiting and parked-state checks.
    /// Live manager phases override graph state except for launch outcomes.
    async fn eval_dep(
        dep_state: &Arc<RwLock<TaskState>>,
        dep_kind: &DependencyKind,
        process_manager: &Arc<NativeProcessManager>,
    ) -> DepEval {
        let dep_guard = dep_state.read().await;
        tracing::trace!(
            "  dep {} status={:?} kind={:?}",
            dep_guard.task.name,
            dep_guard.status,
            dep_kind
        );
        if dep_guard.task.r#type == TaskType::Process {
            let pname = crate::types::process_name(&dep_guard.task.name);
            // Preserve terminal history hidden by an explicit stop.
            let live_phase = process_manager.get_dependency_phase(pname).await;
            let sat = match live_phase {
                // Live phases outrank stale terminal graph state.
                Some(
                    phase @ (ProcessPhase::Waiting
                    | ProcessPhase::Starting
                    | ProcessPhase::Ready
                    | ProcessPhase::Exited
                    | ProcessPhase::GaveUp),
                ) => crate::types::is_process_dep_satisfied(phase, dep_kind),
                // Graph-owned launch outcomes are conclusive for inactive entries.
                phase => match &dep_guard.status {
                    TaskStatus::Completed(_) => {
                        crate::types::is_dep_satisfied(&dep_guard.status, dep_kind)
                    }
                    _ => match phase {
                        Some(p) => crate::types::is_process_dep_satisfied(p, dep_kind),
                        None => DepSatisfaction::NotYet,
                    },
                },
            };
            DepEval {
                task_name: dep_guard.task.name.clone(),
                sat,
                live_phase,
                dep_in_flight: false,
            }
        } else {
            DepEval {
                task_name: dep_guard.task.name.clone(),
                sat: crate::types::is_dep_satisfied(&dep_guard.status, dep_kind),
                live_phase: None,
                dep_in_flight: matches!(
                    dep_guard.status,
                    TaskStatus::Oneshot(OneshotStatus::Running(_))
                ),
            }
        }
    }

    /// Whether every unsatisfied dependency needs external action.
    /// Running and unknown dependencies are conservatively treated as progressing.
    pub async fn dependency_parked(&self, process_name: &str) -> bool {
        let task_name = format!("{PROCESS_TASK_PREFIX}{process_name}");
        self.task_dependency_parked(&task_name).await
    }

    /// Recursive parked-state check over the validated acyclic graph.
    fn task_dependency_parked<'a>(
        &'a self,
        task_name: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + 'a>> {
        Box::pin(async move {
            let Some(&index) = self.task_index_by_name.get(task_name) else {
                return false;
            };
            let mut any_blocker = false;
            for (dep_state, dep_kind) in self.collect_deps(index) {
                let eval = Self::eval_dep(&dep_state, &dep_kind, &self.process_manager).await;
                // Terminal failures settle the waiter; only unresolved edges can park it.
                if eval.sat != DepSatisfaction::NotYet {
                    continue;
                }
                any_blocker = true;
                let parked = match eval.live_phase {
                    Some(ProcessPhase::NotStarted | ProcessPhase::Stopped) => true,
                    Some(ProcessPhase::Waiting) => {
                        self.task_dependency_parked(&eval.task_name).await
                    }
                    Some(_) => false,
                    None if eval.dep_in_flight => false,
                    None => self.task_dependency_parked(&eval.task_name).await,
                };
                if !parked {
                    return false;
                }
            }
            any_blocker
        })
    }

    /// Wait for dependencies; concurrent shutdown takes precedence over failure.
    async fn wait_for_task_deps(
        deps: &[(Arc<RwLock<TaskState>>, DependencyKind)],
        process_manager: &Arc<NativeProcessManager>,
        notify_finished: &Notify,
        shutdown: &tokio_shutdown::Shutdown,
    ) -> DependencyWaitOutcome {
        loop {
            if shutdown.is_cancelled() {
                return DependencyWaitOutcome::Cancelled;
            }

            // Register the notification future BEFORE checking deps to prevent
            // missed wakeups: if a dependency transitions between our check and
            // the await, we will still be woken because the Notified was already
            // registered via enable(). Any manager transition fires
            // notify_finished via task_notify and the per-launch forwarder.
            let notified = notify_finished.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();

            let mut all_satisfied = true;

            for (dep_state, dep_kind) in deps {
                let satisfaction = Self::eval_dep(dep_state, dep_kind, process_manager)
                    .await
                    .sat;
                // eval_dep awaits task and manager locks. Shutdown may have
                // arrived while those were contended; preserve cancellation
                // precedence before interpreting the dependency result.
                if shutdown.is_cancelled() {
                    return DependencyWaitOutcome::Cancelled;
                }
                match satisfaction {
                    DepSatisfaction::Satisfied => {}
                    DepSatisfaction::NeverSatisfiable => {
                        return DependencyWaitOutcome::DependencyFailed;
                    }
                    DepSatisfaction::NotYet => {
                        all_satisfied = false;
                        break;
                    }
                }
            }

            if all_satisfied {
                return DependencyWaitOutcome::Ready;
            }

            tokio::select! {
                biased;
                _ = shutdown.wait_for_shutdown() => {
                    return DependencyWaitOutcome::Cancelled;
                }
                _ = notified => {},
            }
        }
    }

    async fn run_internal(
        &self,
        orchestration_activity: Arc<Activity>,
        register_unscheduled_processes: bool,
    ) -> Outputs {
        // Assign activity IDs upfront for all tasks
        let mut task_ids: HashMap<NodeIndex, u64> = HashMap::new();
        for &index in &self.tasks_order {
            task_ids.insert(index, next_id());
        }

        // Build TaskInfo for all tasks
        let mut task_infos: Vec<TaskInfo> = Vec::new();
        for &index in &self.tasks_order {
            let task_state = self.graph[index].read().await;
            let task_id = task_ids[&index];

            task_infos.push(TaskInfo {
                id: task_id,
                name: task_state.task.name.clone(),
                show_output: task_state.task.show_output,
                is_process: task_state.task.r#type == crate::types::TaskType::Process,
            });
        }

        // Compute hierarchy edges using the extracted function
        let edges = compute_hierarchy_edges(
            &self.graph,
            &self.tasks_order,
            &self.roots,
            &task_ids,
            orchestration_activity.id(),
        );

        // Emit hierarchy once upfront
        emit_task_hierarchy(task_infos, edges);

        let total_tasks = self.tasks_order.len() as u64;
        let completed_tasks = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let scheduled: HashSet<NodeIndex> = self.tasks_order.iter().copied().collect();

        let outputs = Arc::clone(&self.outputs);
        let mut running_tasks = self.shutdown.join_set();

        // Long-lived runners register the full graph for later dynamic starts.
        // Transient task runners expose only their cold schedule.
        let mut process_configs: HashMap<NodeIndex, ProcessConfig> = HashMap::new();
        let mut has_process_tasks = false;
        let process_indices: Vec<_> = if register_unscheduled_processes {
            self.graph.node_indices().collect()
        } else {
            self.tasks_order.clone()
        };
        for index in process_indices {
            if self.shutdown.is_cancelled() {
                break;
            }
            let ts = self.graph[index].read().await;
            if ts.task.r#type != TaskType::Process || ts.task.command.is_none() {
                continue;
            }
            match ts.build_process_config(&self.env, &self.bash, self.supervisor) {
                Ok(mut config) => {
                    has_process_tasks = true;
                    if scheduled.contains(&index) {
                        self.process_manager
                            .register_waiting(config.clone(), Some(orchestration_activity.id()))
                            .await;
                        process_configs.insert(index, config);
                    } else {
                        config.start.enable = false;
                        if let Err(e) = self
                            .process_manager
                            .start_command(&config, Some(orchestration_activity.id()))
                            .await
                        {
                            error!(
                                process = %config.name,
                                error = %e,
                                "failed to register unscheduled process"
                            );
                        }
                    }
                }
                Err(e) => {
                    let name = ts.task.name.clone();
                    drop(ts);
                    let mut ts = self.graph[index].write().await;
                    error!(task = %name, error = %e, "failed to build process config");
                    ts.status = TaskStatus::Completed(TaskCompleted::Failed(
                        std::time::Duration::ZERO,
                        TaskFailure {
                            stdout: Vec::new(),
                            stderr: Vec::new(),
                            error: format!("Failed to build process config: {e}"),
                        },
                    ));
                }
            }
        }

        // External wrappers share a runtime directory and cannot own its API socket.
        if self.supervisor == SupervisionMode::Native
            && has_process_tasks
            && let Err(e) = self.process_manager.start_api_server()
        {
            error!(error = %e, "failed to start process manager API server");
        }

        for index in &self.tasks_order {
            let task_state = &self.graph[*index];
            let task_activity_id = task_ids[index];

            // Check if this is a process task early so we can handle it differently.
            // Process tasks are pre-registered and spawned with background dep checking
            // so they never block the main scheduling loop.
            let is_process_task = {
                let ts = task_state.read().await;
                ts.task.r#type == TaskType::Process
            };

            if self.shutdown.is_cancelled() {
                Self::mark_task_skipped(
                    task_state,
                    task_activity_id,
                    true,
                    &completed_tasks,
                    total_tasks,
                    &orchestration_activity,
                    &self.notify_finished,
                    &self.notify_ui,
                )
                .await;
                continue;
            }

            // Run the task

            if is_process_task {
                // Process task: spawn into background with dependency checking.
                // All process tasks were pre-registered with the process manager,
                // so they already appear in the TUI as "Waiting".
                let config = match process_configs.remove(index) {
                    Some(c) => c,
                    None => {
                        // Pre-registration failed, task already marked as failed
                        Self::signal_task_done(
                            &completed_tasks,
                            total_tasks,
                            &orchestration_activity,
                            &self.notify_finished,
                            &self.notify_ui,
                        );
                        continue;
                    }
                };

                let deps = self.collect_scheduled_deps(*index, &scheduled);

                let task_state_clone = Arc::clone(task_state);
                let notify_finished_clone = Arc::clone(&self.notify_finished);
                let notify_ui_clone = Arc::clone(&self.notify_ui);
                let process_manager_clone = self.process_manager.clone();
                let orchestration_activity_clone = Arc::clone(&orchestration_activity);
                let completed_tasks_clone = Arc::clone(&completed_tasks);
                let shutdown_clone = Arc::clone(&self.shutdown);

                running_tasks.spawn(move || {
                    let orchestration_activity_inner = Arc::clone(&orchestration_activity_clone);

                    async move {
                        // Wait for dependencies in background
                        tracing::debug!(
                            process = %config.name,
                            dependency_count = deps.len(),
                            "waiting for process dependencies"
                        );
                        let dep_outcome = Self::wait_for_task_deps(
                            &deps,
                            &process_manager_clone,
                            &notify_finished_clone,
                            &shutdown_clone,
                        )
                        .await;
                        tracing::debug!(
                            process = %config.name,
                            outcome = ?dep_outcome,
                            "process dependencies resolved"
                        );

                        if dep_outcome != DependencyWaitOutcome::Ready {
                            // Clean up the Waiting entry in the process manager
                            // so the TUI no longer shows this process as "Waiting".
                            process_manager_clone.cancel_waiting(&config.name).await;

                            Self::mark_task_skipped(
                                &task_state_clone,
                                task_activity_id,
                                dep_outcome == DependencyWaitOutcome::Cancelled,
                                &completed_tasks_clone,
                                total_tasks,
                                &orchestration_activity_inner,
                                &notify_finished_clone,
                                &notify_ui_clone,
                            )
                            .await;
                            return;
                        }

                        // Launch the process (pre-registered as Waiting).
                        // The read guard must drop before the Err arm takes
                        // the write lock; a match scrutinee guard lives until
                        // the end of the match and would self-deadlock.
                        let launch_result = {
                            let ts = task_state_clone.read().await;
                            ts.run_process(&process_manager_clone, config).await
                        };
                        let launch_info = match launch_result {
                            Ok(info) => info,
                            Err(e) => {
                                let mut task_state = task_state_clone.write().await;
                                error!(
                                    task = %task_state.task.name,
                                    error = %e,
                                    "failed to start process task"
                                );
                                task_state.status = TaskStatus::Completed(TaskCompleted::Failed(
                                    std::time::Duration::ZERO,
                                    TaskFailure {
                                        stdout: Vec::new(),
                                        stderr: Vec::new(),
                                        error: format!("Failed to start process: {e}"),
                                    },
                                ));
                                Self::signal_task_done(
                                    &completed_tasks_clone,
                                    total_tasks,
                                    &orchestration_activity_inner,
                                    &notify_finished_clone,
                                    &notify_ui_clone,
                                );
                                return;
                            }
                        };

                        if !launch_info.auto_start_off && launch_info.requires_ready_wait {
                            // Stopped/NotStarted end the wait too: a process
                            // stopped mid-launch must not park this task.
                            let _ = wait_for_phase(
                                &process_manager_clone,
                                &notify_finished_clone,
                                &shutdown_clone,
                                &launch_info.process_name,
                                &[
                                    ProcessPhase::Ready,
                                    ProcessPhase::GaveUp,
                                    ProcessPhase::Exited,
                                    ProcessPhase::Stopped,
                                    ProcessPhase::NotStarted,
                                ],
                            )
                            .await;
                        }

                        // Initial setup done; the manager owns the phase from here.
                        Self::signal_task_done(
                            &completed_tasks_clone,
                            total_tasks,
                            &orchestration_activity_inner,
                            &notify_finished_clone,
                            &notify_ui_clone,
                        );
                    }
                    .in_activity(&orchestration_activity_clone)
                });

                continue;
            }

            // Oneshot task: spawn into background with dependency checking,
            // so independent tasks can run in parallel.
            let deps = self.collect_scheduled_deps(*index, &scheduled);

            // TODO: consider Arc-ing self at this point
            let task_state_clone = Arc::clone(task_state);
            let outputs_clone = Arc::clone(&outputs);
            let notify_finished_clone = Arc::clone(&self.notify_finished);
            let notify_ui_clone = Arc::clone(&self.notify_ui);
            // TODO: remove this clone
            let cache = Arc::new(self.cache.clone());
            let shutdown_clone = Arc::clone(&self.shutdown);
            let process_manager_clone = Arc::clone(&self.process_manager);
            let orchestration_activity_clone = Arc::clone(&orchestration_activity);
            let completed_tasks_clone = Arc::clone(&completed_tasks);
            let refresh_task_cache = self.refresh_task_cache;
            let shell_env = self.env.clone();

            running_tasks.spawn(move || {
                // Clone for use inside the async block; the original is borrowed by in_activity
                let orchestration_activity_inner = Arc::clone(&orchestration_activity_clone);

                async move {
                    Self::run_oneshot_task(
                        task_state_clone,
                        deps,
                        outputs_clone,
                        notify_finished_clone.clone(),
                        notify_ui_clone.clone(),
                        cache,
                        shutdown_clone,
                        process_manager_clone,
                        task_activity_id,
                        refresh_task_cache,
                        shell_env,
                    )
                    .await;

                    Self::signal_task_done(
                        &completed_tasks_clone,
                        total_tasks,
                        &orchestration_activity_inner,
                        &notify_finished_clone,
                        &notify_ui_clone,
                    );
                }
                .in_activity(&orchestration_activity_clone)
            });
        }

        // Wait for all tasks to complete
        running_tasks.wait_all().await;

        // wait_all() aborts spawned futures on shutdown so that run_foreground()
        // can proceed to stop_all(). Aborted futures never write back their
        // completion status, so sweep any still-Running tasks to Cancelled.
        if self.shutdown.is_cancelled() {
            for &index in &self.tasks_order {
                let (is_process, task_name, running_oneshot_start) = {
                    let task_state = self.graph[index].read().await;
                    let running_start = match &task_state.status {
                        TaskStatus::Oneshot(OneshotStatus::Running(start)) => Some(*start),
                        _ => None,
                    };
                    let is_pending_process = task_state.task.r#type == TaskType::Process
                        && matches!(task_state.status, TaskStatus::Pending);
                    (
                        is_pending_process,
                        task_state.task.name.clone(),
                        running_start,
                    )
                };

                if let Some(start) = running_oneshot_start {
                    let elapsed = start.elapsed();
                    let mut task_state = self.graph[index].write().await;
                    task_state.status =
                        TaskStatus::Completed(TaskCompleted::Cancelled(Some(elapsed)));
                } else if is_process {
                    // A process never launched (no manager entry) or still live
                    // is cancelled; terminal phases stay Pending and are counted
                    // via the manager in get_completion_status.
                    let phase = self
                        .process_manager
                        .get_phase(crate::types::process_name(&task_name))
                        .await;
                    if matches!(
                        phase,
                        None | Some(
                            ProcessPhase::Waiting | ProcessPhase::Starting | ProcessPhase::Ready
                        )
                    ) {
                        let mut task_state = self.graph[index].write().await;
                        task_state.status = TaskStatus::Completed(TaskCompleted::Cancelled(None));
                    }
                }
            }
        }

        // Check completion status and mark orchestration activity accordingly
        let status = self.get_completion_status().await;

        if status.has_failures() {
            orchestration_activity.fail();
        } else if status.cancelled > 0 {
            orchestration_activity.cancel();
        }

        self.notify_finished.notify_waiters();
        self.notify_ui.notify_one();

        outputs.lock().await.clone()
    }
}

/// The owner-side hooks the process manager delegates to: `ApiRequest::Start`
/// scheduling and the `Wait` parked judgment, both of which need the
/// dependency graph that lives here. Registered via
/// `NativeProcessManager::set_scheduler` (weakly, so the manager never keeps
/// the scheduler alive).
#[async_trait::async_trait]
impl devenv_processes::ProcessScheduler for Tasks {
    async fn start(&self, names: Vec<String>) -> StartOutcome {
        self.start_with_deps(names).await
    }

    async fn dependency_parked(&self, process_name: &str) -> bool {
        Tasks::dependency_parked(self, process_name).await
    }
}

/// Block until the manager reports one of `terminal` phases for `name`.
/// Returns the reached phase, or `None` on shutdown or when the manager has
/// no entry for the process. Event-driven: wakes on `notify_finished`, which
/// the manager fires on every lifecycle and supervisor transition.
async fn wait_for_phase(
    manager: &Arc<NativeProcessManager>,
    notify_finished: &Notify,
    shutdown: &tokio_shutdown::Shutdown,
    name: &str,
    terminal: &[ProcessPhase],
) -> Option<ProcessPhase> {
    loop {
        let notified = notify_finished.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        match manager.get_phase(name).await {
            Some(phase) if terminal.contains(&phase) => return Some(phase),
            None => return None,
            Some(_) => {}
        }
        tokio::select! {
            _ = notified => {}
            _ = shutdown.wait_for_shutdown() => return None,
        }
    }
}

fn process_has_ready_config(task: &crate::TaskConfig) -> bool {
    task.process
        .as_ref()
        .is_some_and(|p| p.has_readiness_probe())
}

/// Compute the hierarchy edges for displaying tasks from task configurations.
///
/// This builds a graph from the task configs and computes the display hierarchy,
/// returning edges as (parent_name, child_name) pairs where root tasks have None
/// as parent.
///
/// # Arguments
/// * `tasks` - The task configurations to process
///
/// # Returns
/// A vector of (Option<parent_name>, child_name) edges for display
pub fn compute_display_hierarchy(tasks: &[crate::TaskConfig]) -> Vec<(Option<String>, String)> {
    use crate::config::parse_dependency;

    if tasks.is_empty() {
        return Vec::new();
    }

    // Build a graph from task configs
    let mut graph: DiGraph<String, ()> = DiGraph::new();
    let mut name_to_index: HashMap<String, NodeIndex> = HashMap::new();

    // Add all tasks as nodes
    for task in tasks {
        let index = graph.add_node(task.name.clone());
        name_to_index.insert(task.name.clone(), index);
    }

    // Add edges for dependencies
    for task in tasks {
        let Some(&task_index) = name_to_index.get(&task.name) else {
            continue;
        };

        // Handle "after" dependencies (task runs after these)
        for dep_name in &task.after {
            if let Ok(dep_spec) = parse_dependency(dep_name)
                && let Some(&dep_index) = name_to_index.get(&dep_spec.name)
            {
                // Edge from dependency to dependent (dep -> task)
                graph.add_edge(dep_index, task_index, ());
            }
        }

        // Handle "before" dependencies (task runs before these)
        for before_name in &task.before {
            if let Ok(dep_spec) = parse_dependency(before_name)
                && let Some(&before_index) = name_to_index.get(&dep_spec.name)
            {
                // Edge from task to the one that runs after (task -> before)
                graph.add_edge(task_index, before_index, ());
            }
        }
    }

    // Find roots (tasks with no dependents - nothing runs after them)
    let roots: Vec<NodeIndex> = graph
        .node_indices()
        .filter(|&index| {
            graph
                .neighbors_directed(index, petgraph::Direction::Outgoing)
                .next()
                .is_none()
        })
        .collect();

    // Get topological order (or just iterate if there are cycles)
    let tasks_order: Vec<NodeIndex> = toposort(&graph, None).unwrap_or_else(|_| {
        // If there's a cycle, just use all nodes in arbitrary order
        graph.node_indices().collect()
    });

    // Compute hierarchy edges using the same algorithm as compute_hierarchy_edges
    let mut edges = Vec::new();

    for &index in &tasks_order {
        let task_name = graph[index].clone();
        let is_root_task = roots.contains(&index);

        if is_root_task {
            edges.push((None, task_name));
        } else {
            // Find dependents (tasks that depend on this task, i.e., run after it)
            let dependents: Vec<NodeIndex> = graph
                .neighbors_directed(index, petgraph::Direction::Outgoing)
                .collect();

            // Filter to uncovered dependents only
            let uncovered_dependents: Vec<NodeIndex> = dependents
                .iter()
                .filter(|&&d1| {
                    // D1 is uncovered if it doesn't transitively depend on any other dependent D2
                    !dependents
                        .iter()
                        .any(|&d2| d1 != d2 && has_path_connecting(&Reversed(&graph), d1, d2, None))
                })
                .copied()
                .collect();

            if uncovered_dependents.is_empty() {
                // Fallback to root if no uncovered dependents
                edges.push((None, task_name));
            } else {
                for dependent_index in uncovered_dependents {
                    let parent_name = graph[dependent_index].clone();
                    edges.push((Some(parent_name), task_name.clone()));
                }
            }
        }
    }

    edges
}

/// Compute the hierarchy edges for displaying tasks in the TUI.
///
/// For each task, this finds its "uncovered" dependents - the most immediate
/// tasks that depend on it. A dependent D1 is "covered" by D2 if D1 transitively
/// depends on D2. We only create edges from uncovered dependents to avoid
/// showing a task under a parent that will also show it through a child.
///
/// # Arguments
/// * `graph` - The task dependency graph (edges point from dependency to dependent)
/// * `tasks_order` - The topological order of tasks to process
/// * `roots` - The slice of root task indices
/// * `task_ids` - Mapping from node index to activity ID
/// * `orchestration_id` - The ID of the orchestration activity (fallback parent)
///
/// # Returns
/// A vector of (parent_id, child_id) edges for the TUI hierarchy
pub fn compute_hierarchy_edges<N, E>(
    graph: &DiGraph<N, E>,
    tasks_order: &[NodeIndex],
    roots: &[NodeIndex],
    task_ids: &HashMap<NodeIndex, u64>,
    orchestration_id: u64,
) -> Vec<(u64, u64)> {
    let mut edges = Vec::new();

    for &index in tasks_order {
        let Some(&task_id) = task_ids.get(&index) else {
            continue;
        };
        let is_root_task = roots.contains(&index);

        if is_root_task {
            edges.push((orchestration_id, task_id));
        } else {
            // Find dependents (tasks that depend on this task)
            let dependents: Vec<NodeIndex> = graph
                .neighbors_directed(index, petgraph::Direction::Outgoing)
                .filter(|dep_index| task_ids.contains_key(dep_index))
                .collect();

            // Filter to uncovered dependents only
            let uncovered_dependents: Vec<NodeIndex> = dependents
                .iter()
                .filter(|&&d1| {
                    // D1 is uncovered if it doesn't transitively depend on any other dependent D2
                    !dependents
                        .iter()
                        .any(|&d2| d1 != d2 && has_path_connecting(&Reversed(graph), d1, d2, None))
                })
                .copied()
                .collect();

            for dependent_index in &uncovered_dependents {
                if let Some(&dependent_id) = task_ids.get(dependent_index) {
                    edges.push((dependent_id, task_id));
                }
            }

            // Fallback to orchestration if no uncovered dependents
            if uncovered_dependents.is_empty() {
                edges.push((orchestration_id, task_id));
            }
        }
    }

    edges
}

#[cfg(test)]
mod schedule_tests {
    use super::*;
    use crate::config::TaskConfig;
    use std::os::unix::fs::PermissionsExt;

    // Keep the TempDir alive with the runtime and cache paths that use it.
    async fn build_test_tasks(
        task_configs: Vec<TaskConfig>,
        roots: Vec<String>,
        ignore_process_deps: bool,
    ) -> (Tasks, tempfile::TempDir) {
        build_test_tasks_with_run_mode(task_configs, roots, RunMode::All, ignore_process_deps).await
    }

    async fn build_test_tasks_with_run_mode(
        task_configs: Vec<TaskConfig>,
        roots: Vec<String>,
        run_mode: RunMode,
        ignore_process_deps: bool,
    ) -> (Tasks, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = tmp.path().to_path_buf();
        let runtime_dir = tmp.path().join("runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();

        let config = Config {
            tasks: task_configs,
            roots,
            run_mode,
            runtime_dir,
            cache_dir,
            sudo_context: None,
            env: HashMap::new(),
            bash: String::new(),
            ignore_process_deps,
            exit_on_idle: Some(false),
            supervisor: devenv_processes::SupervisionMode::Native,
        };

        let shutdown = tokio_shutdown::Shutdown::new();
        let tasks = Tasks::builder(config, VerbosityLevel::Normal, shutdown)
            .build()
            .await
            .unwrap();
        (tasks, tmp)
    }

    fn oneshot_task(name: &str, after: Vec<&str>) -> TaskConfig {
        TaskConfig {
            name: name.to_string(),
            r#type: TaskType::Oneshot,
            after: after.into_iter().map(String::from).collect(),
            command: Some("true".to_string()),
            ..Default::default()
        }
    }

    fn process_task(name: &str, after: Vec<&str>) -> TaskConfig {
        TaskConfig {
            name: name.to_string(),
            r#type: TaskType::Process,
            after: after.into_iter().map(String::from).collect(),
            command: Some("true".to_string()),
            ..Default::default()
        }
    }

    fn executable_script(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn named_pipe(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        nix::unistd::mkfifo(
            &path,
            nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
        )
        .unwrap_or_else(|error| panic!("failed to create {}: {error}", path.display()));
        path
    }

    fn listen_for_pipe_signal(
        path: std::path::PathBuf,
    ) -> tokio::task::JoinHandle<std::io::Result<()>> {
        tokio::task::spawn_blocking(move || {
            use std::io::Read;

            let mut pipe = std::fs::File::open(path)?;
            let mut signal = [0_u8; 1];
            pipe.read_exact(&mut signal)
        })
    }

    async fn wait_for_pipe_signal(
        listener: tokio::task::JoinHandle<std::io::Result<()>>,
        context: &str,
    ) {
        tokio::time::timeout(std::time::Duration::from_secs(10), listener)
            .await
            .unwrap_or_else(|_| panic!("timed out waiting for {context}"))
            .expect("pipe listener panicked")
            .unwrap_or_else(|error| panic!("failed waiting for {context}: {error}"));
    }

    async fn task_names(tasks: &Tasks) -> Vec<String> {
        let mut names = Vec::new();
        for idx in &tasks.tasks_order {
            names.push(tasks.graph[*idx].read().await.task.name.clone());
        }
        names
    }

    #[tokio::test]
    async fn external_supervisor_enforces_builder_invariants() {
        let tmp = tempfile::tempdir().unwrap();
        let config = Config {
            tasks: vec![
                process_task("ns:process:root", vec!["ns:process:dependency@completed"]),
                process_task("ns:process:dependency", vec![]),
            ],
            roots: vec!["ns:process:root".to_string()],
            run_mode: RunMode::All,
            runtime_dir: tmp.path().join("runtime"),
            cache_dir: tmp.path().join("cache"),
            sudo_context: None,
            env: HashMap::new(),
            bash: String::new(),
            ignore_process_deps: false,
            exit_on_idle: None,
            supervisor: SupervisionMode::External,
        };

        let tasks = Tasks::builder(
            config,
            VerbosityLevel::Normal,
            tokio_shutdown::Shutdown::new(),
        )
        .build()
        .await
        .unwrap();

        assert!(tasks.exit_on_idle, "external runners must exit when idle");
        assert!(
            tasks.ignore_process_deps,
            "external runners must leave process ordering to their manager"
        );
        assert_eq!(
            task_names(&tasks).await,
            vec!["ns:process:root"],
            "external runners must not schedule non-root process dependencies"
        );
    }

    #[tokio::test]
    async fn external_supervisor_honors_explicit_idle_override() {
        let tmp = tempfile::tempdir().unwrap();
        let config = Config {
            tasks: vec![],
            roots: vec![],
            run_mode: RunMode::All,
            runtime_dir: tmp.path().join("runtime"),
            cache_dir: tmp.path().join("cache"),
            sudo_context: None,
            env: HashMap::new(),
            bash: String::new(),
            ignore_process_deps: false,
            exit_on_idle: Some(false),
            supervisor: SupervisionMode::External,
        };

        let tasks = Tasks::builder(
            config,
            VerbosityLevel::Normal,
            tokio_shutdown::Shutdown::new(),
        )
        .build()
        .await
        .unwrap();

        assert!(tasks.ignore_process_deps);
        assert!(
            !tasks.exit_on_idle,
            "an explicit external linger override must not be replaced by the default"
        );
    }

    /// A process task running `command`, keyed under the `devenv:processes:`
    /// prefix so `start_with_deps` (which strips that prefix) can find it.
    fn process_task_with_command(name: &str, after: Vec<&str>, command: &str) -> TaskConfig {
        TaskConfig {
            name: format!("{PROCESS_TASK_PREFIX}{name}"),
            r#type: TaskType::Process,
            after: after
                .into_iter()
                .map(|a| format!("{PROCESS_TASK_PREFIX}{a}"))
                .collect(),
            command: Some(command.to_string()),
            ..Default::default()
        }
    }

    fn long_process_task(name: &str, after: Vec<&str>) -> TaskConfig {
        process_task_with_command(name, after, "exec tail -f /dev/null")
    }

    fn self_exit_process_task(name: &str, after: Vec<&str>) -> TaskConfig {
        process_task_with_command(name, after, "echo")
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum ProcessDependency {
        Default,
        Started,
        Ready,
        Completed,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum OneshotDependency {
        Default,
        Started,
        Succeeded,
        Completed,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum TestDependency {
        Process(ProcessDependency),
        Oneshot(OneshotDependency),
    }

    impl TestDependency {
        fn task_type(self) -> TaskType {
            match self {
                Self::Process(_) => TaskType::Process,
                Self::Oneshot(_) => TaskType::Oneshot,
            }
        }

        fn kind(self) -> DependencyKind {
            match self {
                Self::Process(ProcessDependency::Default | ProcessDependency::Ready) => {
                    DependencyKind::Ready
                }
                Self::Process(ProcessDependency::Started)
                | Self::Oneshot(OneshotDependency::Started) => DependencyKind::Started,
                Self::Process(ProcessDependency::Completed)
                | Self::Oneshot(OneshotDependency::Completed) => DependencyKind::Completed,
                Self::Oneshot(OneshotDependency::Default | OneshotDependency::Succeeded) => {
                    DependencyKind::Succeeded
                }
            }
        }

        fn suffix(self) -> Option<&'static str> {
            match self {
                Self::Process(ProcessDependency::Default)
                | Self::Oneshot(OneshotDependency::Default) => None,
                _ => Some(match self.kind() {
                    DependencyKind::Started => "started",
                    DependencyKind::Ready => "ready",
                    DependencyKind::Succeeded => "succeeded",
                    DependencyKind::Completed => "completed",
                }),
            }
        }

        fn allows_dependent(self, exit: TestExit) -> bool {
            matches!(
                self.kind(),
                DependencyKind::Started | DependencyKind::Completed
            ) || exit == TestExit::Success
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum TestExit {
        Success,
        Failure,
    }

    impl TestExit {
        fn code(self) -> i32 {
            match self {
                Self::Success => 0,
                Self::Failure => 7,
            }
        }
    }

    #[derive(Clone, Copy, Debug)]
    struct DependencyCase {
        dependency: TestDependency,
        exit: TestExit,
    }

    const PROCESS_DEPENDENCIES: &[TestDependency] = &[
        TestDependency::Process(ProcessDependency::Default),
        TestDependency::Process(ProcessDependency::Started),
        TestDependency::Process(ProcessDependency::Ready),
        TestDependency::Process(ProcessDependency::Completed),
    ];

    const ONESHOT_DEPENDENCIES: &[TestDependency] = &[
        TestDependency::Oneshot(OneshotDependency::Default),
        TestDependency::Oneshot(OneshotDependency::Started),
        TestDependency::Oneshot(OneshotDependency::Succeeded),
        TestDependency::Oneshot(OneshotDependency::Completed),
    ];

    const TEST_EXITS: &[TestExit] = &[TestExit::Success, TestExit::Failure];

    fn dependency_name(name: &str, suffix: Option<&str>) -> String {
        suffix.map_or_else(|| name.to_string(), |suffix| format!("{name}@{suffix}"))
    }

    fn no_restart_process_config(
        ready_marker: Option<&std::path::Path>,
    ) -> devenv_processes::ProcessConfig {
        devenv_processes::ProcessConfig {
            ready: ready_marker.map(|marker| devenv_processes::ReadyConfig {
                exec: Some(format!("test -f '{}'", marker.to_string_lossy())),
                period: 1,
                ..Default::default()
            }),
            restart: devenv_processes::RestartConfig {
                on: devenv_processes::RestartPolicy::Never,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn external_process_exit_status_distinguishes_success_and_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let roots = vec![
            format!("{PROCESS_TASK_PREFIX}ok"),
            format!("{PROCESS_TASK_PREFIX}fail"),
        ];
        let config = Config {
            tasks: vec![
                process_task_with_command("ok", vec![], "exit 0"),
                process_task_with_command("fail", vec![], "exit 7"),
            ],
            roots: roots.clone(),
            run_mode: RunMode::All,
            runtime_dir: tmp.path().join("runtime"),
            cache_dir: tmp.path().join("cache"),
            sudo_context: None,
            env: HashMap::new(),
            bash: String::new(),
            ignore_process_deps: false,
            exit_on_idle: None,
            supervisor: SupervisionMode::External,
        };
        let tasks = Tasks::builder(
            config,
            VerbosityLevel::Normal,
            tokio_shutdown::Shutdown::new(),
        )
        .build()
        .await
        .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(10), tasks.run(true))
            .await
            .expect("external processes did not settle");
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            tasks.process_manager().run_foreground(
                tokio_util::sync::CancellationToken::new(),
                None,
                devenv_processes::OnIdle::Exit,
            ),
        )
        .await
        .expect("external manager did not become idle")
        .unwrap();

        let status = tasks.get_completion_status().await;
        assert_eq!(status.succeeded, 1);
        assert_eq!(status.failed, 1);
        tasks.process_manager().stop_all().await.unwrap();
    }

    async fn run_cold_dependency_case(case: DependencyCase) {
        let case_name = format!("{case:?}");
        let files = tempfile::tempdir().unwrap();
        let source_started = files.path().join("source-started");
        let source_ready = files.path().join("source-ready");
        let source_finished = files.path().join("source-finished");
        let downstream_ran = files.path().join("downstream-ran");
        let release_gate = named_pipe(files.path(), "release-gate");
        let process_hold = named_pipe(files.path(), "process-hold");

        let (source, downstream, source_name, downstream_name) = match case.dependency.task_type() {
            TaskType::Process => {
                let source_name = format!("{PROCESS_TASK_PREFIX}source");
                let downstream_name = "test:downstream".to_string();
                let source_body = match (
                    case.dependency.kind(),
                    case.dependency.allows_dependent(case.exit),
                ) {
                    (DependencyKind::Started, _) => format!(
                        "touch '{started}'\nread _ < '{gate}'\n\
                         touch '{finished}'\nexit {exit}",
                        started = source_started.to_string_lossy(),
                        gate = release_gate.to_string_lossy(),
                        finished = source_finished.to_string_lossy(),
                        exit = case.exit.code(),
                    ),
                    (DependencyKind::Ready, true) => format!(
                        "touch '{started}'\ntouch '{ready}'\nread _ < '{gate}'\n\
                         touch '{finished}'\nread _ < '{hold}'",
                        started = source_started.to_string_lossy(),
                        ready = source_ready.to_string_lossy(),
                        gate = release_gate.to_string_lossy(),
                        finished = source_finished.to_string_lossy(),
                        hold = process_hold.to_string_lossy(),
                    ),
                    (DependencyKind::Ready, false) | (DependencyKind::Completed, _) => format!(
                        "touch '{started}'\ntouch '{finished}'\nexit {exit}",
                        started = source_started.to_string_lossy(),
                        finished = source_finished.to_string_lossy(),
                        exit = case.exit.code(),
                    ),
                    (DependencyKind::Succeeded, _) => unreachable!(),
                };
                let source_script = executable_script(files.path(), "process-source", &source_body);
                let mut source =
                    process_task_with_command("source", vec![], &source_script.to_string_lossy());
                source.process = Some(no_restart_process_config(
                    (case.dependency.kind() == DependencyKind::Ready)
                        .then_some(source_ready.as_path()),
                ));

                let required = match case.dependency.kind() {
                    DependencyKind::Started => None,
                    DependencyKind::Ready => Some(&source_ready),
                    DependencyKind::Completed => Some(&source_finished),
                    DependencyKind::Succeeded => unreachable!(),
                };
                let validate = required.map_or_else(String::new, |required| {
                    format!("test -f '{}' || exit 91\n", required.to_string_lossy())
                });
                let release = matches!(
                    case.dependency.kind(),
                    DependencyKind::Started | DependencyKind::Ready
                )
                .then(|| {
                    format!(
                        "printf 'released\\n' > '{}'\n",
                        release_gate.to_string_lossy()
                    )
                })
                .unwrap_or_default();
                let downstream_script = executable_script(
                    files.path(),
                    "task-downstream",
                    &format!(
                        "{validate}touch '{ran}'\n{release}",
                        ran = downstream_ran.to_string_lossy(),
                    ),
                );
                let mut downstream = oneshot_task(&downstream_name, vec![]);
                downstream.after = vec![dependency_name(&source_name, case.dependency.suffix())];
                downstream.command = Some(downstream_script.to_string_lossy().into_owned());

                (source, downstream, source_name, downstream_name)
            }
            TaskType::Oneshot => {
                let source_name = "test:source".to_string();
                let downstream_name = format!("{PROCESS_TASK_PREFIX}downstream");
                let source_body = match case.dependency.kind() {
                    DependencyKind::Started => format!(
                        "touch '{started}'\nread _ < '{gate}'\n\
                         touch '{finished}'\nexit {exit}",
                        started = source_started.to_string_lossy(),
                        gate = release_gate.to_string_lossy(),
                        finished = source_finished.to_string_lossy(),
                        exit = case.exit.code(),
                    ),
                    DependencyKind::Succeeded | DependencyKind::Completed => format!(
                        "touch '{started}'\ntouch '{finished}'\nexit {exit}",
                        started = source_started.to_string_lossy(),
                        finished = source_finished.to_string_lossy(),
                        exit = case.exit.code(),
                    ),
                    DependencyKind::Ready => unreachable!(),
                };
                let source_script = executable_script(files.path(), "oneshot-source", &source_body);
                let mut source = oneshot_task(&source_name, vec![]);
                source.command = Some(source_script.to_string_lossy().into_owned());

                let required = match case.dependency.kind() {
                    DependencyKind::Started => None,
                    DependencyKind::Succeeded | DependencyKind::Completed => Some(&source_finished),
                    DependencyKind::Ready => unreachable!(),
                };
                let validate = required.map_or_else(String::new, |required| {
                    format!("test -f '{}' || exit 92\n", required.to_string_lossy())
                });
                let release = if case.dependency.kind() == DependencyKind::Started {
                    format!(
                        "printf 'released\\n' > '{}'\n",
                        release_gate.to_string_lossy()
                    )
                } else {
                    String::new()
                };
                let downstream_script = executable_script(
                    files.path(),
                    "process-downstream",
                    &format!(
                        "{validate}touch '{ran}'\n{release}read _ < '{hold}'",
                        ran = downstream_ran.to_string_lossy(),
                        hold = process_hold.to_string_lossy(),
                    ),
                );
                let mut downstream = process_task_with_command(
                    "downstream",
                    vec![],
                    &downstream_script.to_string_lossy(),
                );
                downstream.after = vec![dependency_name(&source_name, case.dependency.suffix())];
                downstream.process = Some(no_restart_process_config(Some(&downstream_ran)));

                (source, downstream, source_name, downstream_name)
            }
        };

        let (tasks, _tmp) = build_test_tasks(
            vec![source, downstream, long_process_task("unrelated", vec![])],
            vec![downstream_name.clone()],
            false,
        )
        .await;
        let scheduled = task_names(&tasks).await;
        assert_eq!(
            scheduled.len(),
            2,
            "{}: root closure must contain exactly the heterogeneous edge",
            case_name
        );
        assert!(
            scheduled.contains(&source_name),
            "{}: source missing",
            case_name
        );
        assert!(
            scheduled.contains(&downstream_name),
            "{}: downstream missing",
            case_name
        );
        assert!(
            !scheduled.contains(&format!("{PROCESS_TASK_PREFIX}unrelated")),
            "{}: unrelated process entered the root closure",
            case_name
        );

        let run_result =
            tokio::time::timeout(std::time::Duration::from_secs(10), tasks.run(true)).await;
        if run_result.is_err() {
            let source_status = tasks.graph[tasks.task_index_by_name[&source_name]]
                .read()
                .await
                .status
                .clone();
            let downstream_status = tasks.graph[tasks.task_index_by_name[&downstream_name]]
                .read()
                .await
                .status
                .clone();
            let source_phase = tasks.process_manager().get_phase("source").await;
            let downstream_phase = tasks.process_manager().get_phase("downstream").await;
            tasks.process_manager().stop_all().await.unwrap();
            panic!(
                "{}: heterogeneous run did not settle; markers=({},{},{},{}), \
                 statuses=({source_status:?},{downstream_status:?}), \
                 phases=({source_phase:?},{downstream_phase:?})",
                case_name,
                source_started.exists(),
                source_ready.exists(),
                source_finished.exists(),
                downstream_ran.exists(),
            );
        }

        let downstream_status = tasks.graph[tasks.task_index_by_name[&downstream_name]]
            .read()
            .await
            .status
            .clone();
        let completion = tasks.get_completion_status().await;
        let downstream_phase = if case.dependency.task_type() == TaskType::Oneshot {
            tasks.process_manager().get_phase("downstream").await
        } else {
            None
        };

        tasks.process_manager().stop_all().await.unwrap();

        assert_eq!(
            downstream_ran.exists(),
            case.dependency.allows_dependent(case.exit),
            "{}: downstream execution disagreed with dependency semantics; \
             status={downstream_status:?}, phase={downstream_phase:?}, completion={completion:?}",
            case_name,
        );
        if case.dependency.allows_dependent(case.exit) {
            if case.dependency.task_type() == TaskType::Oneshot {
                assert_eq!(
                    downstream_phase,
                    Some(ProcessPhase::Ready),
                    "{}: downstream process never became ready",
                    case_name
                );
            } else {
                assert!(
                    matches!(
                        downstream_status,
                        TaskStatus::Completed(TaskCompleted::Success(_, _))
                    ),
                    "{}: expected downstream success, got {downstream_status:?}",
                    case_name
                );
            }
        } else {
            assert!(
                matches!(
                    downstream_status,
                    TaskStatus::Completed(TaskCompleted::DependencyFailed)
                ),
                "{}: hard dependency failure did not propagate: {downstream_status:?}",
                case_name
            );
            assert!(
                completion.has_failures(),
                "{}: failure was hidden",
                case_name
            );
        }

        if case.dependency.kind() == DependencyKind::Completed && case.exit.code() != 0 {
            assert!(
                !completion.has_failures(),
                "{}: @completed must keep its source failure soft",
                case_name
            );
        }
        if case.dependency.task_type() == TaskType::Oneshot {
            assert_eq!(
                tasks.process_manager().get_phase("downstream").await,
                Some(ProcessPhase::Stopped),
                "{}: downstream process was not cleaned up",
                case_name
            );
        } else {
            let phase = tasks.process_manager().get_phase("source").await;
            assert!(
                matches!(phase, Some(ProcessPhase::Stopped | ProcessPhase::Exited)),
                "{}: source process did not reach a terminal phase: {phase:?}",
                case_name
            );
        }
    }

    #[tokio::test]
    async fn cold_start_covers_every_process_dependency_condition() {
        for &dependency in PROCESS_DEPENDENCIES {
            for &exit in TEST_EXITS {
                run_cold_dependency_case(DependencyCase { dependency, exit }).await;
            }
        }
    }

    #[tokio::test]
    async fn cold_start_covers_every_oneshot_dependency_condition() {
        for &dependency in ONESHOT_DEPENDENCIES {
            for &exit in TEST_EXITS {
                run_cold_dependency_case(DependencyCase { dependency, exit }).await;
            }
        }
    }

    #[tokio::test]
    async fn completed_process_that_gives_up_is_a_soft_failure() {
        let files = tempfile::tempdir().unwrap();
        let downstream_ran = files.path().join("downstream-ran");
        let source_script = executable_script(files.path(), "source", "exit 7");
        let downstream_script = executable_script(
            files.path(),
            "downstream",
            &format!("touch '{}'", downstream_ran.to_string_lossy()),
        );

        let mut source =
            process_task_with_command("source", vec![], &source_script.to_string_lossy());
        source.process = Some(devenv_processes::ProcessConfig {
            restart: devenv_processes::RestartConfig {
                on: devenv_processes::RestartPolicy::OnFailure,
                max: Some(0),
                window: None,
            },
            ..Default::default()
        });
        let downstream_name = "test:downstream";
        let mut downstream = oneshot_task(downstream_name, vec![]);
        downstream.after = vec![format!("{PROCESS_TASK_PREFIX}source@completed")];
        downstream.command = Some(downstream_script.to_string_lossy().into_owned());

        let (tasks, _tmp) = build_test_tasks(
            vec![source, downstream],
            vec![downstream_name.to_string()],
            false,
        )
        .await;
        tokio::time::timeout(std::time::Duration::from_secs(10), tasks.run(true))
            .await
            .expect("gave-up @completed dependency did not settle");

        assert!(downstream_ran.exists());
        assert_eq!(
            tasks.process_manager().get_phase("source").await,
            Some(ProcessPhase::GaveUp)
        );
        let completion = tasks.get_completion_status().await;
        assert_eq!(completion.failed, 1);
        assert_eq!(completion.soft_failed, 1);
        assert!(!completion.has_failures());
        tasks.process_manager().stop_all().await.unwrap();
    }

    async fn run_dynamic_oneshot_dependency_case(case: DependencyCase) {
        let case_name = format!("{case:?}");
        assert_eq!(case.dependency.task_type(), TaskType::Oneshot);
        let files = tempfile::tempdir().unwrap();
        let source_started = files.path().join("source-started");
        let source_finished = files.path().join("source-finished");
        let downstream_ran = files.path().join("downstream-ran");
        let source_gate = named_pipe(files.path(), "source-gate");
        let alpha_hold = named_pipe(files.path(), "alpha-hold");
        let downstream_hold = named_pipe(files.path(), "downstream-hold");

        let alpha_script = executable_script(
            files.path(),
            "alpha",
            &format!("read _ < '{}'", alpha_hold.to_string_lossy()),
        );
        let alpha = process_task_with_command("alpha", vec![], &alpha_script.to_string_lossy());

        let source_name = "test:source";
        let source_body = match case.dependency.kind() {
            DependencyKind::Started => format!(
                "touch '{started}'\nread _ < '{gate}'\n\
                 touch '{finished}'\nexit {exit}",
                started = source_started.to_string_lossy(),
                gate = source_gate.to_string_lossy(),
                finished = source_finished.to_string_lossy(),
                exit = case.exit.code(),
            ),
            DependencyKind::Succeeded | DependencyKind::Completed => format!(
                "touch '{started}'\ntouch '{finished}'\nexit {exit}",
                started = source_started.to_string_lossy(),
                finished = source_finished.to_string_lossy(),
                exit = case.exit.code(),
            ),
            DependencyKind::Ready => unreachable!(),
        };
        let source_script = executable_script(files.path(), "source", &source_body);
        let mut source = oneshot_task(source_name, vec![]);
        source.command = Some(source_script.to_string_lossy().into_owned());

        let required = match case.dependency.kind() {
            DependencyKind::Started => None,
            DependencyKind::Succeeded | DependencyKind::Completed => Some(&source_finished),
            DependencyKind::Ready => unreachable!(),
        };
        let validate = required.map_or_else(String::new, |required| {
            format!("test -f '{}' || exit 92\n", required.to_string_lossy())
        });
        let release = if case.dependency.kind() == DependencyKind::Started {
            format!(
                "printf 'released\\n' > '{}'\n",
                source_gate.to_string_lossy()
            )
        } else {
            String::new()
        };
        let downstream_script = executable_script(
            files.path(),
            "downstream",
            &format!(
                "{validate}touch '{ran}'\n{release}read _ < '{hold}'",
                ran = downstream_ran.to_string_lossy(),
                hold = downstream_hold.to_string_lossy(),
            ),
        );
        let mut downstream =
            process_task_with_command("downstream", vec![], &downstream_script.to_string_lossy());
        downstream.after = vec![dependency_name(source_name, case.dependency.suffix())];
        downstream.process = Some(no_restart_process_config(Some(&downstream_ran)));

        let (tasks, _tmp) = build_test_tasks_with_run_mode(
            vec![alpha, source, downstream],
            vec![format!("{PROCESS_TASK_PREFIX}alpha")],
            RunMode::Before,
            false,
        )
        .await;
        let tasks = Arc::new(tasks);
        let scheduler: Arc<dyn devenv_processes::ProcessScheduler> = tasks.clone();
        tasks
            .process_manager()
            .set_scheduler(Arc::downgrade(&scheduler));

        tasks.run(true).await;
        wait_phase(&tasks, "alpha", ProcessPhase::Ready).await;
        assert!(
            !tasks
                .tasks_order
                .contains(&tasks.task_index_by_name[source_name]),
            "{}: source must begin outside the cold schedule",
            case_name
        );

        let outcome = tasks.start_with_deps(["downstream"]).await;
        assert_eq!(
            outcome.scheduled,
            ["downstream"],
            "{}: dynamic root was not scheduled",
            case_name
        );

        wait_task_completed(&tasks, source_name).await;
        if case.dependency.allows_dependent(case.exit) {
            wait_phase(&tasks, "downstream", ProcessPhase::Ready).await;
        } else {
            wait_task_completed(&tasks, &format!("{PROCESS_TASK_PREFIX}downstream")).await;
        }

        let source_status = tasks.graph[tasks.task_index_by_name[source_name]]
            .read()
            .await
            .status
            .clone();
        let downstream_status = tasks.graph
            [tasks.task_index_by_name[&format!("{PROCESS_TASK_PREFIX}downstream")]]
            .read()
            .await
            .status
            .clone();
        tasks.process_manager().stop_all().await.unwrap();

        assert_eq!(
            downstream_ran.exists(),
            case.dependency.allows_dependent(case.exit),
            "{}: dynamic downstream disagreed with dependency semantics",
            case_name
        );
        if case.exit.code() == 0 {
            assert!(matches!(
                source_status,
                TaskStatus::Completed(TaskCompleted::Success(_, _))
            ));
        } else {
            assert!(matches!(
                source_status,
                TaskStatus::Completed(TaskCompleted::Failed(_, _))
            ));
        }
        if case.dependency.allows_dependent(case.exit) {
            assert_eq!(
                tasks.process_manager().get_phase("downstream").await,
                Some(ProcessPhase::Stopped),
                "{}: dynamic downstream was not cleaned up",
                case_name
            );
        } else {
            assert!(
                matches!(
                    downstream_status,
                    TaskStatus::Completed(TaskCompleted::DependencyFailed)
                ),
                "{}: dynamic dependency failure did not propagate: {downstream_status:?}",
                case_name
            );
        }
    }

    #[tokio::test]
    async fn dynamic_start_covers_every_oneshot_dependency_condition() {
        for &dependency in ONESHOT_DEPENDENCIES {
            for &exit in TEST_EXITS {
                run_dynamic_oneshot_dependency_case(DependencyCase { dependency, exit }).await;
            }
        }
    }

    async fn run_dynamic_process_dependency_case(case: DependencyCase) {
        let case_name = format!("{case:?}");
        assert_eq!(case.dependency.task_type(), TaskType::Process);
        let files = tempfile::tempdir().unwrap();
        let source_started = files.path().join("source-started");
        let source_ready = files.path().join("source-ready");
        let source_finished = files.path().join("source-finished");
        let bridge_ran = files.path().join("bridge-ran");
        let downstream_ran = files.path().join("downstream-ran");
        let source_gate = named_pipe(files.path(), "source-gate");
        let alpha_hold = named_pipe(files.path(), "alpha-hold");
        let source_hold = named_pipe(files.path(), "source-hold");
        let downstream_hold = named_pipe(files.path(), "downstream-hold");

        let alpha_script = executable_script(
            files.path(),
            "alpha",
            &format!("read _ < '{}'", alpha_hold.to_string_lossy()),
        );
        let alpha = process_task_with_command("alpha", vec![], &alpha_script.to_string_lossy());

        let source_body = match (
            case.dependency.kind(),
            case.dependency.allows_dependent(case.exit),
        ) {
            (DependencyKind::Started, _) => format!(
                "touch '{started}'\nread _ < '{gate}'\n\
                 touch '{finished}'\nexit {exit}",
                started = source_started.to_string_lossy(),
                gate = source_gate.to_string_lossy(),
                finished = source_finished.to_string_lossy(),
                exit = case.exit.code(),
            ),
            (DependencyKind::Ready, true) => format!(
                "touch '{started}'\ntouch '{ready}'\nread _ < '{gate}'\n\
                 touch '{finished}'\nread _ < '{hold}'",
                started = source_started.to_string_lossy(),
                ready = source_ready.to_string_lossy(),
                gate = source_gate.to_string_lossy(),
                finished = source_finished.to_string_lossy(),
                hold = source_hold.to_string_lossy(),
            ),
            (DependencyKind::Ready, false) | (DependencyKind::Completed, _) => format!(
                "touch '{started}'\ntouch '{finished}'\nexit {exit}",
                started = source_started.to_string_lossy(),
                finished = source_finished.to_string_lossy(),
                exit = case.exit.code(),
            ),
            (DependencyKind::Succeeded, _) => unreachable!(),
        };
        let source_script = executable_script(files.path(), "source", &source_body);
        let mut source =
            process_task_with_command("source", vec![], &source_script.to_string_lossy());
        source.process = Some(no_restart_process_config(
            (case.dependency.kind() == DependencyKind::Ready).then_some(source_ready.as_path()),
        ));

        let source_stays_not_started =
            case.dependency.kind() == DependencyKind::Completed && case.exit.code() == 0;
        let source_name = format!("{PROCESS_TASK_PREFIX}source");
        let bridge_name = "test:bridge";
        let required = match case.dependency.kind() {
            DependencyKind::Started => None,
            DependencyKind::Ready => Some(&source_ready),
            DependencyKind::Completed if source_stays_not_started => None,
            DependencyKind::Completed => Some(&source_finished),
            DependencyKind::Succeeded => unreachable!(),
        };
        let validate = required.map_or_else(String::new, |required| {
            format!("test -f '{}' || exit 91\n", required.to_string_lossy())
        });
        let release = matches!(
            case.dependency.kind(),
            DependencyKind::Started | DependencyKind::Ready
        )
        .then(|| {
            format!(
                "printf 'released\\n' > '{}'\n",
                source_gate.to_string_lossy()
            )
        })
        .unwrap_or_default();
        let bridge_script = executable_script(
            files.path(),
            "bridge",
            &format!(
                "{validate}touch '{bridge}'\n{release}",
                bridge = bridge_ran.to_string_lossy(),
            ),
        );
        let mut bridge = oneshot_task(bridge_name, vec![]);
        bridge.after = vec![dependency_name(&source_name, case.dependency.suffix())];
        bridge.command = Some(bridge_script.to_string_lossy().into_owned());

        let downstream_script = executable_script(
            files.path(),
            "downstream",
            &format!(
                "test -f '{bridge}' || exit 92\ntouch '{ran}'\nread _ < '{hold}'",
                bridge = bridge_ran.to_string_lossy(),
                ran = downstream_ran.to_string_lossy(),
                hold = downstream_hold.to_string_lossy(),
            ),
        );
        let downstream_name = format!("{PROCESS_TASK_PREFIX}downstream");
        let mut downstream =
            process_task_with_command("downstream", vec![], &downstream_script.to_string_lossy());
        downstream.after = vec![format!("{bridge_name}@succeeded")];
        downstream.process = Some(no_restart_process_config(Some(&downstream_ran)));

        let (tasks, _tmp) = build_test_tasks_with_run_mode(
            vec![alpha, source, bridge, downstream],
            vec![format!("{PROCESS_TASK_PREFIX}alpha")],
            RunMode::Before,
            false,
        )
        .await;
        let tasks = Arc::new(tasks);
        let scheduler: Arc<dyn devenv_processes::ProcessScheduler> = tasks.clone();
        tasks
            .process_manager()
            .set_scheduler(Arc::downgrade(&scheduler));

        tasks.run(true).await;
        wait_phase(&tasks, "alpha", ProcessPhase::Ready).await;

        if case.dependency.kind() == DependencyKind::Completed && !source_stays_not_started {
            let outcome = tasks.start_with_deps(["source"]).await;
            assert_eq!(outcome.scheduled, ["source"]);
            wait_phase(&tasks, "source", ProcessPhase::Exited).await;
        }

        let outcome = tasks.start_with_deps(["downstream"]).await;
        assert_eq!(
            outcome.scheduled,
            ["downstream"],
            "{}: dynamic root was not scheduled",
            case_name
        );

        if case.dependency.kind() != DependencyKind::Completed {
            assert_eq!(
                tasks.process_manager().get_phase("downstream").await,
                Some(ProcessPhase::Waiting),
                "{}: downstream must wait while its process predecessor is not started",
                case_name
            );
            let outcome = tasks.start_with_deps(["source"]).await;
            assert_eq!(outcome.scheduled, ["source"]);
        }

        wait_task_completed(&tasks, bridge_name).await;
        if case.dependency.allows_dependent(case.exit) {
            wait_phase(&tasks, "downstream", ProcessPhase::Ready).await;
        } else {
            wait_task_completed(&tasks, &downstream_name).await;
        }

        let bridge_status = tasks.graph[tasks.task_index_by_name[bridge_name]]
            .read()
            .await
            .status
            .clone();
        let downstream_status = tasks.graph[tasks.task_index_by_name[&downstream_name]]
            .read()
            .await
            .status
            .clone();
        let source_phase = tasks.process_manager().get_phase("source").await;
        tasks.process_manager().stop_all().await.unwrap();

        assert_eq!(
            downstream_ran.exists(),
            case.dependency.allows_dependent(case.exit),
            "{}: dynamic mixed chain disagreed with dependency semantics",
            case_name
        );
        if case.dependency.allows_dependent(case.exit) {
            assert!(
                matches!(
                    bridge_status,
                    TaskStatus::Completed(TaskCompleted::Success(_, _))
                ),
                "{}: bridge did not succeed: {bridge_status:?}",
                case_name
            );
            assert_eq!(
                tasks.process_manager().get_phase("downstream").await,
                Some(ProcessPhase::Stopped),
                "{}: downstream was not cleaned up",
                case_name
            );
            if source_stays_not_started {
                assert_eq!(
                    source_phase,
                    Some(ProcessPhase::NotStarted),
                    "{}: @completed should not force-start its process dependency",
                    case_name
                );
            }
        } else {
            assert!(matches!(
                bridge_status,
                TaskStatus::Completed(TaskCompleted::DependencyFailed)
            ));
            assert!(matches!(
                downstream_status,
                TaskStatus::Completed(TaskCompleted::DependencyFailed)
            ));
        }
    }

    #[tokio::test]
    async fn dynamic_start_covers_every_process_dependency_condition() {
        for &dependency in PROCESS_DEPENDENCIES {
            for &exit in TEST_EXITS {
                run_dynamic_process_dependency_case(DependencyCase { dependency, exit }).await;
            }
        }
    }

    async fn run_heterogeneous_diamond(dynamic: bool) {
        let files = tempfile::tempdir().unwrap();
        let source_ready = files.path().join("source-ready");
        let left_runs = files.path().join("left-runs");
        let right_runs = files.path().join("right-runs");
        let backend_ran = files.path().join("backend-ran");
        let unrelated_ran = files.path().join("unrelated-ran");
        let source_hold = named_pipe(files.path(), "source-hold");
        let backend_hold = named_pipe(files.path(), "backend-hold");
        let alpha_hold = named_pipe(files.path(), "alpha-hold");

        let source_script = executable_script(
            files.path(),
            "source",
            &format!(
                "touch '{ready}'\nread _ < '{hold}'",
                ready = source_ready.to_string_lossy(),
                hold = source_hold.to_string_lossy(),
            ),
        );
        let mut source =
            process_task_with_command("source", vec![], &source_script.to_string_lossy());
        source.process = Some(no_restart_process_config(Some(&source_ready)));

        let mut left = oneshot_task("test:left", vec![]);
        left.after = vec![format!("{PROCESS_TASK_PREFIX}source@ready")];
        left.command = Some(
            executable_script(
                files.path(),
                "left",
                &format!("printf 'left\\n' >> '{}'", left_runs.to_string_lossy()),
            )
            .to_string_lossy()
            .into_owned(),
        );
        let mut right = oneshot_task("test:right", vec![]);
        right.after = vec![format!("{PROCESS_TASK_PREFIX}source")];
        right.command = Some(
            executable_script(
                files.path(),
                "right",
                &format!("printf 'right\\n' >> '{}'", right_runs.to_string_lossy()),
            )
            .to_string_lossy()
            .into_owned(),
        );

        let backend_script = executable_script(
            files.path(),
            "backend",
            &format!(
                "test -f '{left}' || exit 91\n\
                 test -f '{right}' || exit 92\n\
                 touch '{ran}'\nread _ < '{hold}'",
                left = left_runs.to_string_lossy(),
                right = right_runs.to_string_lossy(),
                ran = backend_ran.to_string_lossy(),
                hold = backend_hold.to_string_lossy(),
            ),
        );
        let mut backend =
            process_task_with_command("backend", vec![], &backend_script.to_string_lossy());
        backend.after = vec![
            "test:left@succeeded".to_string(),
            "test:right@succeeded".to_string(),
        ];
        backend.process = Some(no_restart_process_config(Some(&backend_ran)));

        let unrelated_script = executable_script(
            files.path(),
            "unrelated",
            &format!("touch '{}'", unrelated_ran.to_string_lossy()),
        );
        let unrelated =
            process_task_with_command("unrelated", vec![], &unrelated_script.to_string_lossy());

        let mut configs = vec![source, left, right, backend, unrelated];
        if dynamic {
            let alpha_script = executable_script(
                files.path(),
                "alpha",
                &format!("read _ < '{}'", alpha_hold.to_string_lossy()),
            );
            configs.push(process_task_with_command(
                "alpha",
                vec![],
                &alpha_script.to_string_lossy(),
            ));
        }
        let root = if dynamic { "alpha" } else { "backend" };
        let run_mode = if dynamic {
            RunMode::Before
        } else {
            RunMode::All
        };
        let (tasks, _tmp) = build_test_tasks_with_run_mode(
            configs,
            vec![format!("{PROCESS_TASK_PREFIX}{root}")],
            run_mode,
            false,
        )
        .await;
        let tasks = Arc::new(tasks);
        let scheduler: Arc<dyn devenv_processes::ProcessScheduler> = tasks.clone();
        tasks
            .process_manager()
            .set_scheduler(Arc::downgrade(&scheduler));

        if !dynamic {
            let scheduled = task_names(&tasks).await;
            assert_eq!(scheduled.len(), 4);
            assert!(!scheduled.contains(&format!("{PROCESS_TASK_PREFIX}unrelated")));
        }

        tasks.run(true).await;
        if dynamic {
            wait_phase(&tasks, "alpha", ProcessPhase::Ready).await;
            let outcome = tasks.start_with_deps(["backend"]).await;
            assert_eq!(outcome.scheduled, ["backend"]);
            assert_eq!(
                tasks.process_manager().get_phase("backend").await,
                Some(ProcessPhase::Waiting)
            );
            let outcome = tasks.start_with_deps(["source"]).await;
            assert_eq!(outcome.scheduled, ["source"]);
        }
        wait_phase(&tasks, "backend", ProcessPhase::Ready).await;

        tasks.process_manager().stop_all().await.unwrap();
        assert_eq!(std::fs::read_to_string(&left_runs).unwrap(), "left\n");
        assert_eq!(std::fs::read_to_string(&right_runs).unwrap(), "right\n");
        assert!(backend_ran.exists());
        assert!(!unrelated_ran.exists());
        for process in ["source", "backend"] {
            assert_eq!(
                tasks.process_manager().get_phase(process).await,
                Some(ProcessPhase::Stopped),
                "{process} was not cleaned up"
            );
        }
    }

    #[tokio::test]
    async fn heterogeneous_diamond_is_deduplicated_on_cold_and_dynamic_start() {
        run_heterogeneous_diamond(false).await;
        run_heterogeneous_diamond(true).await;
    }

    #[tokio::test]
    async fn shutdown_cancels_cold_heterogeneous_closure() {
        let files = tempfile::tempdir().unwrap();
        let source_hold = named_pipe(files.path(), "source-hold");
        let bridge_hold = named_pipe(files.path(), "bridge-hold");
        let bridge_started = named_pipe(files.path(), "bridge-started");
        let backend_hold = named_pipe(files.path(), "backend-hold");
        let source_ready = files.path().join("source-ready");
        let source_script = executable_script(
            files.path(),
            "source",
            &format!(
                "touch '{ready}'\nread _ < '{hold}'",
                ready = source_ready.to_string_lossy(),
                hold = source_hold.to_string_lossy(),
            ),
        );
        let mut source =
            process_task_with_command("source", vec![], &source_script.to_string_lossy());
        source.process = Some(no_restart_process_config(Some(&source_ready)));

        let mut bridge = oneshot_task("test:bridge", vec![]);
        bridge.after = vec![format!("{PROCESS_TASK_PREFIX}source@ready")];
        bridge.command = Some(
            executable_script(
                files.path(),
                "bridge",
                &format!(
                    "printf x > '{started}'\nread _ < '{hold}'",
                    started = bridge_started.to_string_lossy(),
                    hold = bridge_hold.to_string_lossy(),
                ),
            )
            .to_string_lossy()
            .into_owned(),
        );
        let backend_script = executable_script(
            files.path(),
            "backend",
            &format!("read _ < '{}'", backend_hold.to_string_lossy()),
        );
        let mut backend =
            process_task_with_command("backend", vec![], &backend_script.to_string_lossy());
        backend.after = vec!["test:bridge@succeeded".to_string()];

        let (tasks, _tmp) = build_test_tasks(
            vec![source, bridge, backend],
            vec![format!("{PROCESS_TASK_PREFIX}backend")],
            false,
        )
        .await;
        let tasks = Arc::new(tasks);
        let bridge_listener = listen_for_pipe_signal(bridge_started);
        let running = Arc::clone(&tasks);
        let run = tokio::spawn(async move { running.run(true).await });

        wait_for_pipe_signal(bridge_listener, "bridge command to start").await;
        assert_eq!(
            tasks.process_manager().get_phase("source").await,
            Some(ProcessPhase::Ready)
        );
        assert_eq!(
            tasks.process_manager().get_phase("backend").await,
            Some(ProcessPhase::Waiting)
        );
        tasks.shutdown.shutdown();
        tokio::time::timeout(std::time::Duration::from_secs(10), run)
            .await
            .expect("cancelled heterogeneous run did not settle")
            .expect("heterogeneous run task panicked");

        for name in [
            format!("{PROCESS_TASK_PREFIX}source"),
            "test:bridge".to_string(),
            format!("{PROCESS_TASK_PREFIX}backend"),
        ] {
            let status = tasks.graph[tasks.task_index_by_name[&name]]
                .read()
                .await
                .status
                .clone();
            assert!(
                matches!(status, TaskStatus::Completed(TaskCompleted::Cancelled(_))),
                "{name} did not record cancellation: {status:?}"
            );
        }
        tasks.process_manager().stop_all().await.unwrap();
        for process in ["source", "backend"] {
            assert_eq!(
                tasks.process_manager().get_phase(process).await,
                Some(ProcessPhase::Stopped)
            );
        }
        assert!(tasks.process_manager().wait_settled().await);
    }

    #[tokio::test]
    async fn cold_start_runs_process_oneshot_process_chain_in_order() {
        let scripts = tempfile::tempdir().unwrap();
        let source_ready = scripts.path().join("source-ready");
        let bridge_ran = scripts.path().join("bridge-ran");
        let backend_ran = scripts.path().join("backend-ran");
        let ordering_violation = scripts.path().join("ordering-violation");
        let source_hold = named_pipe(scripts.path(), "source-hold");
        let backend_hold = named_pipe(scripts.path(), "backend-hold");

        let source_script = executable_script(
            scripts.path(),
            "source",
            &format!(
                "touch '{ready}'\nread _ < '{hold}'",
                ready = source_ready.to_string_lossy(),
                hold = source_hold.to_string_lossy(),
            ),
        );
        let bridge_script = executable_script(
            scripts.path(),
            "bridge",
            &format!(
                "test -f '{}' || {{ touch '{}'; exit 21; }}\ntouch '{}'",
                source_ready.to_string_lossy(),
                ordering_violation.to_string_lossy(),
                bridge_ran.to_string_lossy()
            ),
        );
        let backend_script = executable_script(
            scripts.path(),
            "backend",
            &format!(
                "test -f '{}' || {{ touch '{}'; exit 22; }}\n\
                 touch '{}'\nread _ < '{}'",
                bridge_ran.to_string_lossy(),
                ordering_violation.to_string_lossy(),
                backend_ran.to_string_lossy(),
                backend_hold.to_string_lossy(),
            ),
        );

        let mut source =
            process_task_with_command("source", vec![], &source_script.to_string_lossy());
        source.process = Some(devenv_processes::ProcessConfig {
            ready: Some(devenv_processes::ReadyConfig {
                exec: Some(format!("test -f '{}'", source_ready.to_string_lossy())),
                period: 1,
                ..Default::default()
            }),
            ..Default::default()
        });

        let bridge_name = "devenv:tasks:bridge";
        let mut bridge = oneshot_task(bridge_name, vec![]);
        bridge.after = vec![format!("{PROCESS_TASK_PREFIX}source@ready")];
        bridge.command = Some(bridge_script.to_string_lossy().into_owned());

        let mut backend =
            process_task_with_command("backend", vec![], &backend_script.to_string_lossy());
        backend.after = vec![format!("{bridge_name}@succeeded")];
        backend.process = Some(devenv_processes::ProcessConfig {
            ready: Some(devenv_processes::ReadyConfig {
                exec: Some(format!("test -f '{}'", backend_ran.to_string_lossy())),
                period: 1,
                ..Default::default()
            }),
            ..Default::default()
        });

        let (tasks, _tmp) = build_test_tasks(
            vec![
                source,
                bridge,
                backend,
                long_process_task("unrelated", vec![]),
            ],
            vec![format!("{PROCESS_TASK_PREFIX}backend")],
            false,
        )
        .await;

        let scheduled = task_names(&tasks).await;
        assert_eq!(scheduled.len(), 3);
        assert!(scheduled.contains(&format!("{PROCESS_TASK_PREFIX}source")));
        assert!(scheduled.contains(&bridge_name.to_string()));
        assert!(scheduled.contains(&format!("{PROCESS_TASK_PREFIX}backend")));
        assert!(!scheduled.contains(&format!("{PROCESS_TASK_PREFIX}unrelated")));

        tokio::time::timeout(std::time::Duration::from_secs(10), tasks.run(true))
            .await
            .expect("mixed process/task chain did not settle");

        assert!(source_ready.exists(), "source never became ready");
        assert!(bridge_ran.exists(), "bridge task never ran");
        assert!(backend_ran.exists(), "backend process never launched");
        assert!(
            !ordering_violation.exists(),
            "a downstream node ran before its dependency"
        );
        assert_eq!(
            tasks.process_manager().get_phase("source").await,
            Some(ProcessPhase::Ready)
        );
        assert_eq!(
            tasks.process_manager().get_phase("backend").await,
            Some(ProcessPhase::Ready)
        );
        assert!(matches!(
            &tasks.graph[tasks.task_index_by_name[bridge_name]]
                .read()
                .await
                .status,
            TaskStatus::Completed(TaskCompleted::Success(_, _))
        ));

        tasks.process_manager().stop_all().await.unwrap();
    }

    #[tokio::test]
    async fn cold_start_blocks_process_after_failing_oneshot() {
        let scripts = tempfile::tempdir().unwrap();
        let bridge_ran = scripts.path().join("failing-bridge-ran");
        let backend_ran = scripts.path().join("blocked-backend-ran");
        let source_hold = named_pipe(scripts.path(), "source-hold");
        let backend_hold = named_pipe(scripts.path(), "backend-hold");

        let source_script = executable_script(
            scripts.path(),
            "failure-source",
            &format!("read _ < '{}'", source_hold.to_string_lossy()),
        );
        let bridge_script = executable_script(
            scripts.path(),
            "failing-bridge",
            &format!("touch '{}'\nexit 23", bridge_ran.to_string_lossy()),
        );
        let backend_script = executable_script(
            scripts.path(),
            "blocked-backend",
            &format!(
                "touch '{}'\nread _ < '{}'",
                backend_ran.to_string_lossy(),
                backend_hold.to_string_lossy(),
            ),
        );

        let mut source =
            process_task_with_command("failure-source", vec![], &source_script.to_string_lossy());
        source.process = Some(devenv_processes::ProcessConfig {
            ready: Some(devenv_processes::ReadyConfig {
                exec: Some("true".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        });

        let bridge_name = "devenv:tasks:failing-bridge";
        let mut bridge = oneshot_task(bridge_name, vec![]);
        bridge.after = vec![format!("{PROCESS_TASK_PREFIX}failure-source@ready")];
        bridge.command = Some(bridge_script.to_string_lossy().into_owned());

        let mut backend =
            process_task_with_command("blocked-backend", vec![], &backend_script.to_string_lossy());
        backend.after = vec![format!("{bridge_name}@succeeded")];

        let backend_name = format!("{PROCESS_TASK_PREFIX}blocked-backend");
        let (tasks, _tmp) = build_test_tasks(
            vec![source, bridge, backend],
            vec![backend_name.clone()],
            false,
        )
        .await;

        tokio::time::timeout(std::time::Duration::from_secs(10), tasks.run(true))
            .await
            .expect("failing mixed process/task chain did not settle");

        assert!(bridge_ran.exists(), "failing bridge task never ran");
        assert!(
            !backend_ran.exists(),
            "backend launched despite a failed task dependency"
        );
        assert!(matches!(
            &tasks.graph[tasks.task_index_by_name[bridge_name]]
                .read()
                .await
                .status,
            TaskStatus::Completed(TaskCompleted::Failed(_, _))
        ));
        assert!(matches!(
            &tasks.graph[tasks.task_index_by_name[&backend_name]]
                .read()
                .await
                .status,
            TaskStatus::Completed(TaskCompleted::DependencyFailed)
        ));
        assert_eq!(
            tasks.process_manager().get_phase("blocked-backend").await,
            Some(ProcessPhase::Stopped)
        );
        assert!(tasks.get_completion_status().await.has_failures());

        tasks.process_manager().stop_all().await.unwrap();
    }

    /// Wait for a lifecycle notification that publishes the requested phase.
    async fn wait_phase(tasks: &Tasks, name: &str, want: ProcessPhase) {
        tokio::time::timeout(std::time::Duration::from_secs(60), async {
            loop {
                let notified = tasks.notify_finished.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if tasks.process_manager().get_phase(name).await == Some(want) {
                    return;
                }
                notified.await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for process {name} to reach {want:?}"))
    }

    async fn wait_task_completed(tasks: &Tasks, name: &str) {
        let index = tasks.task_index_by_name[name];
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let notified = tasks.notify_finished.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if matches!(
                    tasks.graph[index].read().await.status,
                    TaskStatus::Completed(_)
                ) {
                    return;
                }
                notified.await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for task {name} to complete"));
    }

    #[tokio::test]
    async fn dependency_wait_reports_failure_without_shutdown() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(NativeProcessManager::new(temp_dir.path().to_path_buf()).unwrap());
        let dependency = Arc::new(RwLock::new(TaskState::new(
            oneshot_task("devenv:tasks:failed", vec![]),
            VerbosityLevel::Normal,
            None,
        )));
        dependency.write().await.status = TaskStatus::Completed(TaskCompleted::DependencyFailed);
        let deps = vec![(dependency, DependencyKind::Succeeded)];
        let shutdown = tokio_shutdown::Shutdown::new();

        assert_eq!(
            Tasks::wait_for_task_deps(&deps, &manager, &Notify::new(), &shutdown).await,
            DependencyWaitOutcome::DependencyFailed
        );
    }

    #[tokio::test]
    async fn dependency_wait_prefers_shutdown_over_failure() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(NativeProcessManager::new(temp_dir.path().to_path_buf()).unwrap());
        let dependency = Arc::new(RwLock::new(TaskState::new(
            oneshot_task("devenv:tasks:failed", vec![]),
            VerbosityLevel::Normal,
            None,
        )));
        dependency.write().await.status = TaskStatus::Completed(TaskCompleted::DependencyFailed);
        let deps = vec![(dependency, DependencyKind::Succeeded)];
        let shutdown = tokio_shutdown::Shutdown::new();
        shutdown.shutdown();

        assert_eq!(
            Tasks::wait_for_task_deps(&deps, &manager, &Notify::new(), &shutdown).await,
            DependencyWaitOutcome::Cancelled
        );
    }

    #[tokio::test]
    async fn start_with_deps_relaunches_stopped_process() {
        let (tasks, _tmp) = build_test_tasks(
            vec![long_process_task("web", vec![])],
            vec![format!("{PROCESS_TASK_PREFIX}web")],
            false,
        )
        .await;

        tasks.run(true).await;

        wait_phase(&tasks, "web", ProcessPhase::Ready).await;

        tasks.process_manager().stop_and_keep("web").await.unwrap();
        assert_eq!(
            tasks.process_manager().get_phase("web").await,
            Some(ProcessPhase::Stopped)
        );

        let outcome = tasks.start_with_deps(["web"]).await;
        assert_eq!(outcome.scheduled, vec!["web".to_string()]);
        wait_phase(&tasks, "web", ProcessPhase::Ready).await;

        tasks.process_manager().stop_all().await.unwrap();
    }

    #[tokio::test]
    async fn start_with_deps_relaunches_exited_process() {
        let marker_dir = tempfile::tempdir().unwrap();
        let marker = marker_dir.path().join("ran");
        let relaunched = marker_dir.path().join("relaunched");
        let hold = named_pipe(marker_dir.path(), "hold");
        let exec = format!(
            "if [ -e '{marker}' ]; then touch '{relaunched}'; read _ < '{hold}'; \
             else touch '{marker}'; fi",
            marker = marker.display(),
            relaunched = relaunched.display(),
            hold = hold.display(),
        );
        let task = TaskConfig {
            name: format!("{PROCESS_TASK_PREFIX}web"),
            r#type: TaskType::Process,
            command: Some(exec),
            process: Some(no_restart_process_config(Some(&relaunched))),
            ..Default::default()
        };

        let (tasks, _tmp) =
            build_test_tasks(vec![task], vec![format!("{PROCESS_TASK_PREFIX}web")], false).await;

        tasks.run(true).await;

        wait_phase(&tasks, "web", ProcessPhase::Exited).await;

        let outcome = tasks.start_with_deps(["web"]).await;
        assert_eq!(outcome.scheduled, vec!["web".to_string()]);
        wait_phase(&tasks, "web", ProcessPhase::Ready).await;

        tasks.process_manager().stop_all().await.unwrap();
    }

    #[tokio::test]
    async fn start_with_deps_relaunches_gave_up_process() {
        let marker_dir = tempfile::tempdir().unwrap();
        let marker = marker_dir.path().join("runs");
        let ready = marker_dir.path().join("ready");
        let hold = named_pipe(marker_dir.path(), "hold");
        let exec = format!(
            "count=$(cat '{m}' 2>/dev/null || echo 0); \
             count=$((count + 1)); echo \"$count\" > '{m}'; \
             if [ \"$count\" -le 2 ]; then exit 1; fi; \
             touch '{ready}'; read _ < '{hold}'",
            m = marker.display(),
            ready = ready.display(),
            hold = hold.display(),
        );
        let mut process = no_restart_process_config(Some(&ready));
        process.restart = devenv_processes::config::RestartConfig {
            on: devenv_processes::config::RestartPolicy::OnFailure,
            max: Some(1),
            window: None,
        };
        let task = TaskConfig {
            name: format!("{PROCESS_TASK_PREFIX}web"),
            r#type: TaskType::Process,
            command: Some(exec),
            process: Some(process),
            ..Default::default()
        };

        let (tasks, _tmp) =
            build_test_tasks(vec![task], vec![format!("{PROCESS_TASK_PREFIX}web")], false).await;

        tasks.run(true).await;
        wait_phase(&tasks, "web", ProcessPhase::GaveUp).await;

        let outcome = tasks.start_with_deps(["web"]).await;
        assert_eq!(outcome.scheduled, vec!["web".to_string()]);
        wait_phase(&tasks, "web", ProcessPhase::Ready).await;
        assert_eq!(std::fs::read_to_string(&marker).unwrap().trim(), "3");

        tasks.process_manager().stop_all().await.unwrap();
    }

    #[tokio::test]
    async fn start_with_deps_waits_for_unsatisfied_dependency() {
        let (tasks, _tmp) = build_test_tasks(
            vec![
                long_process_task("gamma", vec![]),
                long_process_task("beta", vec!["gamma@started"]),
            ],
            vec![
                format!("{PROCESS_TASK_PREFIX}gamma"),
                format!("{PROCESS_TASK_PREFIX}beta"),
            ],
            false,
        )
        .await;

        tasks.run(true).await;
        wait_phase(&tasks, "beta", ProcessPhase::Ready).await;

        tasks.process_manager().stop_and_keep("beta").await.unwrap();
        tasks
            .process_manager()
            .stop_and_keep("gamma")
            .await
            .unwrap();

        let outcome = tasks.start_with_deps(["beta"]).await;
        assert_eq!(outcome.scheduled, vec!["beta".to_string()]);

        let phase = tasks
            .process_manager
            .get_phase("beta")
            .await
            .expect("beta must stay registered while its dependency is unmet");
        assert_eq!(
            phase,
            ProcessPhase::Waiting,
            "beta must wait for its gamma dependency, not launch"
        );
        assert!(
            tasks
                .process_manager
                .subscribe_status("beta")
                .await
                .is_none(),
            "beta must not have an active supervisor while gamma is down"
        );

        let outcome = tasks.start_with_deps(["gamma"]).await;
        assert_eq!(outcome.scheduled, ["gamma"]);
        wait_phase(&tasks, "beta", ProcessPhase::Ready).await;

        tasks.process_manager().stop_all().await.unwrap();
    }

    #[tokio::test]
    async fn start_with_deps_runs_unseen_oneshot_dependency_closure() {
        let prepare = "devenv:tasks:prepare";
        let setup = "devenv:tasks:setup";
        let scripts = tempfile::tempdir().unwrap();
        let prepare_script = scripts.path().join("prepare");
        let setup_script = scripts.path().join("setup");
        for script in [&prepare_script, &setup_script] {
            std::fs::write(script, "#!/bin/sh\nexit 0\n").unwrap();
            std::fs::set_permissions(script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let mut prepare_task = oneshot_task(prepare, vec![]);
        prepare_task.command = Some(prepare_script.to_string_lossy().into_owned());
        let mut setup_task = oneshot_task(setup, vec![prepare]);
        setup_task.command = Some(setup_script.to_string_lossy().into_owned());
        let mut beta = long_process_task("beta", vec![]);
        beta.after = vec![setup.to_string()];

        let (tasks, _tmp) = build_test_tasks_with_run_mode(
            vec![
                long_process_task("alpha", vec![]),
                prepare_task,
                setup_task,
                beta,
            ],
            vec![format!("{PROCESS_TASK_PREFIX}alpha")],
            RunMode::Before,
            false,
        )
        .await;
        let tasks = Arc::new(tasks);
        let scheduler: Arc<dyn devenv_processes::ProcessScheduler> = tasks.clone();
        tasks
            .process_manager()
            .set_scheduler(Arc::downgrade(&scheduler));

        tasks.run(true).await;
        wait_phase(&tasks, "alpha", ProcessPhase::Ready).await;

        for name in [prepare, setup] {
            let index = tasks.task_index_by_name[name];
            assert!(
                !tasks.tasks_order.contains(&index),
                "{name} must be outside the cold schedule"
            );
            assert!(
                matches!(tasks.graph[index].read().await.status, TaskStatus::Pending),
                "{name} must start unscheduled"
            );
        }

        let outcome = tasks.start_with_deps(["beta"]).await;
        assert_eq!(outcome.scheduled, vec!["beta".to_string()]);

        for name in [prepare, setup] {
            wait_task_completed(&tasks, name).await;
            let index = tasks.task_index_by_name[name];
            let status = tasks.graph[index].read().await.status.clone();
            assert!(
                matches!(&status, TaskStatus::Completed(TaskCompleted::Success(_, _))),
                "{name} must run to completion before beta launches, got {status:?}"
            );
        }
        wait_phase(&tasks, "beta", ProcessPhase::Ready).await;
        assert!(
            tasks.process_manager().wait_settled().await,
            "processes wait must settle after the dynamic closure completes"
        );

        tasks.process_manager().stop_all().await.unwrap();
    }

    #[tokio::test]
    async fn failing_unseen_oneshot_propagates_to_dynamic_process_closure() {
        let failing = "devenv:tasks:failing-setup";
        let scripts = tempfile::tempdir().unwrap();
        let failing_script = scripts.path().join("failing-setup");
        std::fs::write(&failing_script, "#!/bin/sh\nexit 23\n").unwrap();
        std::fs::set_permissions(&failing_script, std::fs::Permissions::from_mode(0o755)).unwrap();

        let mut failing_setup = oneshot_task(failing, vec![]);
        failing_setup.command = Some(failing_script.to_string_lossy().into_owned());
        let mut beta = long_process_task("beta", vec![]);
        beta.after = vec![failing.to_string()];
        beta.process = Some(devenv_processes::config::ProcessConfig {
            ready: Some(devenv_processes::config::ReadyConfig {
                exec: Some("true".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        });
        let gamma = long_process_task("gamma", vec!["beta@ready"]);

        // Only alpha belongs to the cold schedule. The failing one-shot and
        // both dependent processes are discovered by the later dynamic start.
        let (tasks, _tmp) = build_test_tasks_with_run_mode(
            vec![
                long_process_task("alpha", vec![]),
                failing_setup,
                beta,
                gamma,
            ],
            vec![format!("{PROCESS_TASK_PREFIX}alpha")],
            RunMode::Before,
            false,
        )
        .await;
        let tasks = Arc::new(tasks);
        let scheduler: Arc<dyn devenv_processes::ProcessScheduler> = tasks.clone();
        tasks
            .process_manager()
            .set_scheduler(Arc::downgrade(&scheduler));

        tasks.run(true).await;
        wait_phase(&tasks, "alpha", ProcessPhase::Ready).await;

        let outcome = tasks.start_with_deps(["beta", "gamma"]).await;
        assert_eq!(outcome.scheduled, ["beta", "gamma"]);

        for name in [
            failing,
            &format!("{PROCESS_TASK_PREFIX}beta"),
            &format!("{PROCESS_TASK_PREFIX}gamma"),
        ] {
            wait_task_completed(&tasks, name).await;
        }

        let failing_status = tasks.graph[tasks.task_index_by_name[failing]]
            .read()
            .await
            .status
            .clone();
        assert!(
            matches!(
                failing_status,
                TaskStatus::Completed(TaskCompleted::Failed(_, _))
            ),
            "failing setup must retain its Failed graph result: {failing_status:?}"
        );

        for name in ["beta", "gamma"] {
            let task_name = format!("{PROCESS_TASK_PREFIX}{name}");
            let status = tasks.graph[tasks.task_index_by_name[&task_name]]
                .read()
                .await
                .status
                .clone();
            assert!(
                matches!(
                    status,
                    TaskStatus::Completed(TaskCompleted::DependencyFailed)
                ),
                "{name} must record DependencyFailed, got {status:?}"
            );
            assert_eq!(
                tasks.process_manager().get_phase(name).await,
                Some(ProcessPhase::Stopped),
                "{name} must leave Waiting when its dependency fails"
            );
        }
        assert!(
            tasks.process_manager().wait_settled().await,
            "no dynamically scheduled node may remain pending or waiting"
        );

        tasks.process_manager().stop_all().await.unwrap();
    }

    #[tokio::test]
    async fn shutdown_cancels_dynamic_process_closure() {
        let setup = "devenv:tasks:long-setup";
        let scripts = tempfile::tempdir().unwrap();
        let setup_hold = named_pipe(scripts.path(), "setup-hold");
        let setup_started = named_pipe(scripts.path(), "setup-started");
        let alpha_hold = named_pipe(scripts.path(), "alpha-hold");
        let setup_script = executable_script(
            scripts.path(),
            "long-setup",
            &format!(
                "printf x > '{started}'\nread _ < '{hold}'",
                started = setup_started.to_string_lossy(),
                hold = setup_hold.to_string_lossy(),
            ),
        );
        let alpha_script = executable_script(
            scripts.path(),
            "alpha",
            &format!("read _ < '{}'", alpha_hold.to_string_lossy()),
        );

        let mut long_setup = oneshot_task(setup, vec![]);
        long_setup.command = Some(setup_script.to_string_lossy().into_owned());
        let mut beta = long_process_task("beta", vec![]);
        beta.after = vec![setup.to_string()];
        beta.process = Some(devenv_processes::config::ProcessConfig {
            ready: Some(devenv_processes::config::ReadyConfig {
                exec: Some("true".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        });
        let gamma = long_process_task("gamma", vec!["beta@ready"]);

        let (tasks, _tmp) = build_test_tasks_with_run_mode(
            vec![
                process_task_with_command("alpha", vec![], &alpha_script.to_string_lossy()),
                long_setup,
                beta,
                gamma,
            ],
            vec![format!("{PROCESS_TASK_PREFIX}alpha")],
            RunMode::Before,
            false,
        )
        .await;
        let tasks = Arc::new(tasks);
        let setup_listener = listen_for_pipe_signal(setup_started);
        let scheduler: Arc<dyn devenv_processes::ProcessScheduler> = tasks.clone();
        tasks
            .process_manager()
            .set_scheduler(Arc::downgrade(&scheduler));

        tasks.run(true).await;
        wait_phase(&tasks, "alpha", ProcessPhase::Ready).await;

        let outcome = tasks.start_with_deps(["beta", "gamma"]).await;
        assert_eq!(outcome.scheduled, ["beta", "gamma"]);

        wait_for_pipe_signal(setup_listener, "dynamic setup command to start").await;

        tasks.shutdown.shutdown();

        for name in [
            setup.to_string(),
            format!("{PROCESS_TASK_PREFIX}beta"),
            format!("{PROCESS_TASK_PREFIX}gamma"),
        ] {
            wait_task_completed(&tasks, &name).await;
            let status = tasks.graph[tasks.task_index_by_name[&name]]
                .read()
                .await
                .status
                .clone();
            assert!(
                matches!(status, TaskStatus::Completed(TaskCompleted::Cancelled(_))),
                "{name} must record Cancelled on shutdown, got {status:?}"
            );
        }
        for name in ["beta", "gamma"] {
            assert_eq!(
                tasks.process_manager().get_phase(name).await,
                Some(ProcessPhase::Stopped)
            );
        }
        assert!(tasks.process_manager().wait_settled().await);

        tasks.process_manager().stop_all().await.unwrap();
    }

    #[tokio::test]
    async fn start_with_deps_classifies_names() {
        let (tasks, _tmp) = build_test_tasks(
            vec![long_process_task("web", vec![])],
            vec![format!("{PROCESS_TASK_PREFIX}web")],
            false,
        )
        .await;

        tasks.run(true).await;
        wait_phase(&tasks, "web", ProcessPhase::Ready).await;

        let outcome = tasks.start_with_deps(["web"]).await;
        assert_eq!(outcome.skipped, vec!["web".to_string()]);
        assert!(outcome.scheduled.is_empty());
        assert!(outcome.unknown.is_empty());
        assert!(outcome.failed.is_empty());

        let outcome = tasks.start_with_deps(["nosuch"]).await;
        assert_eq!(outcome.unknown, vec!["nosuch".to_string()]);
        assert!(outcome.scheduled.is_empty());
        assert!(outcome.skipped.is_empty());

        tasks.process_manager().stop_and_keep("web").await.unwrap();
        let outcome = tasks.start_with_deps(["web"]).await;
        assert_eq!(outcome.scheduled, vec!["web".to_string()]);
        assert!(outcome.skipped.is_empty());
        wait_phase(&tasks, "web", ProcessPhase::Ready).await;

        tasks.process_manager().stop_all().await.unwrap();
    }

    #[tokio::test]
    async fn concurrent_dynamic_starts_launch_each_process_and_oneshot_once() {
        let scripts = tempfile::tempdir().unwrap();
        let setup_count = scripts.path().join("setup-count");
        let beta_count = scripts.path().join("beta-count");
        let gamma_count = scripts.path().join("gamma-count");

        let setup_script = scripts.path().join("setup");
        std::fs::write(
            &setup_script,
            format!(
                "#!/bin/sh\necho setup >> '{}'\n",
                setup_count.to_string_lossy()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&setup_script, std::fs::Permissions::from_mode(0o755)).unwrap();

        let setup_name = "devenv:tasks:shared-setup";
        let mut setup = oneshot_task(setup_name, vec![]);
        setup.command = Some(setup_script.to_string_lossy().into_owned());

        let mut beta = process_task_with_command(
            "beta",
            vec![],
            &format!(
                "echo beta >> '{}'; exec tail -f /dev/null",
                beta_count.to_string_lossy()
            ),
        );
        beta.after = vec![setup_name.to_string()];
        beta.process = Some(no_restart_process_config(Some(&beta_count)));
        let mut gamma = process_task_with_command(
            "gamma",
            vec![],
            &format!(
                "echo gamma >> '{}'; exec tail -f /dev/null",
                gamma_count.to_string_lossy()
            ),
        );
        gamma.after = vec![setup_name.to_string()];
        gamma.process = Some(no_restart_process_config(Some(&gamma_count)));

        let (tasks, _tmp) = build_test_tasks_with_run_mode(
            vec![long_process_task("alpha", vec![]), setup, beta, gamma],
            vec![format!("{PROCESS_TASK_PREFIX}alpha")],
            RunMode::Before,
            false,
        )
        .await;
        let tasks = Arc::new(tasks);
        let scheduler: Arc<dyn devenv_processes::ProcessScheduler> = tasks.clone();
        tasks
            .process_manager()
            .set_scheduler(Arc::downgrade(&scheduler));

        tasks.run(true).await;
        wait_phase(&tasks, "alpha", ProcessPhase::Ready).await;

        let first_tasks = Arc::clone(&tasks);
        let second_tasks = Arc::clone(&tasks);
        let (first, second) = tokio::join!(
            async move { first_tasks.start_with_deps(["beta", "gamma", "beta"]).await },
            async move { second_tasks.start_with_deps(["beta"]).await },
        );

        let beta_scheduled = first
            .scheduled
            .iter()
            .chain(&second.scheduled)
            .filter(|name| name.as_str() == "beta")
            .count();
        let beta_skipped = first
            .skipped
            .iter()
            .chain(&second.skipped)
            .filter(|name| name.as_str() == "beta")
            .count();
        assert_eq!(beta_scheduled, 1);
        assert_eq!(beta_skipped, 1);
        assert_eq!(
            first
                .scheduled
                .iter()
                .chain(&second.scheduled)
                .filter(|name| name.as_str() == "gamma")
                .count(),
            1
        );
        assert!(first.failed.is_empty() && second.failed.is_empty());

        wait_phase(&tasks, "beta", ProcessPhase::Ready).await;
        wait_phase(&tasks, "gamma", ProcessPhase::Ready).await;
        wait_task_completed(&tasks, setup_name).await;

        for (path, expected) in [
            (&setup_count, "setup"),
            (&beta_count, "beta"),
            (&gamma_count, "gamma"),
        ] {
            let contents = std::fs::read_to_string(path).unwrap();
            assert_eq!(
                contents.lines().collect::<Vec<_>>(),
                [expected],
                "{} must launch exactly once",
                path.display()
            );
        }

        tasks.process_manager().stop_all().await.unwrap();
    }

    #[tokio::test]
    async fn dependency_parked_judges_live_and_transitive() {
        let (tasks, _tmp) = build_test_tasks(
            vec![
                long_process_task("delta", vec![]),
                long_process_task("gamma", vec!["delta@started"]),
                long_process_task("beta", vec!["gamma@started"]),
            ],
            vec![
                format!("{PROCESS_TASK_PREFIX}delta"),
                format!("{PROCESS_TASK_PREFIX}gamma"),
                format!("{PROCESS_TASK_PREFIX}beta"),
            ],
            false,
        )
        .await;
        let tasks = Arc::new(tasks);
        let scheduler: Arc<dyn devenv_processes::ProcessScheduler> = tasks.clone();
        tasks
            .process_manager()
            .set_scheduler(Arc::downgrade(&scheduler));

        tasks.run(true).await;
        wait_phase(&tasks, "delta", ProcessPhase::Ready).await;
        wait_phase(&tasks, "gamma", ProcessPhase::Ready).await;
        wait_phase(&tasks, "beta", ProcessPhase::Ready).await;

        assert!(!tasks.dependency_parked("beta").await);
        assert!(tasks.process_manager().wait_settled().await);

        tasks.process_manager().stop_and_keep("beta").await.unwrap();
        tasks
            .process_manager()
            .stop_and_keep("gamma")
            .await
            .unwrap();
        tasks
            .process_manager()
            .stop_and_keep("delta")
            .await
            .unwrap();

        let outcome = tasks.start_with_deps(["beta"]).await;
        assert_eq!(outcome.scheduled, vec!["beta".to_string()]);
        assert_eq!(
            tasks.process_manager().get_phase("beta").await,
            Some(ProcessPhase::Waiting)
        );
        assert!(
            tasks.dependency_parked("beta").await,
            "beta must be parked: gamma is stopped"
        );
        assert!(
            tasks.process_manager().wait_settled().await,
            "a parked Waiting process must settle Wait"
        );

        let outcome = tasks.start_with_deps(["gamma"]).await;
        assert_eq!(outcome.scheduled, vec!["gamma".to_string()]);
        assert!(
            tasks.dependency_parked("gamma").await,
            "gamma must be parked: delta is stopped"
        );
        assert!(
            tasks.dependency_parked("beta").await,
            "beta must be transitively parked through waiting gamma"
        );
        assert!(tasks.process_manager().wait_settled().await);

        let outcome = tasks.start_with_deps(["delta"]).await;
        assert_eq!(outcome.scheduled, vec!["delta".to_string()]);
        wait_phase(&tasks, "beta", ProcessPhase::Ready).await;
        assert!(!tasks.dependency_parked("beta").await);
        assert!(!tasks.dependency_parked("gamma").await);

        tasks.process_manager().stop_all().await.unwrap();
    }

    #[tokio::test]
    async fn dependency_parked_does_not_park_on_running_oneshot() {
        let mut migrate = oneshot_task("devenv:tasks:migrate", vec![]);
        migrate.after = vec![format!("{PROCESS_TASK_PREFIX}d@started")];
        let mut p = long_process_task("p", vec![]);
        p.after = vec!["devenv:tasks:migrate@succeeded".to_string()];

        let (tasks, _tmp) = build_test_tasks(
            vec![long_process_task("d", vec![]), migrate, p],
            vec![
                format!("{PROCESS_TASK_PREFIX}d"),
                "devenv:tasks:migrate".to_string(),
                format!("{PROCESS_TASK_PREFIX}p"),
            ],
            false,
        )
        .await;

        let d_idx = tasks.task_index_by_name[&format!("{PROCESS_TASK_PREFIX}d")];
        let d_cfg = tasks.graph[d_idx]
            .read()
            .await
            .build_process_config(&tasks.env, &tasks.bash, tasks.supervisor)
            .unwrap();
        tasks.process_manager().register_waiting(d_cfg, None).await;
        tasks.process_manager().cancel_waiting("d").await;
        assert_eq!(
            tasks.process_manager().get_phase("d").await,
            Some(ProcessPhase::Stopped)
        );

        let o_idx = tasks.task_index_by_name["devenv:tasks:migrate"];
        tasks.graph[o_idx].write().await.status =
            TaskStatus::Oneshot(OneshotStatus::Running(tokio::time::Instant::now()));

        assert!(
            !tasks.dependency_parked("p").await,
            "a process waiting on a running oneshot must not be judged parked, \
             even if a process the oneshot depended on was since stopped"
        );
    }

    #[tokio::test]
    async fn dependency_on_started_survives_explicit_stop_of_self_exited_process() {
        let p = self_exit_process_task("p", vec![]);
        let mut d = long_process_task("d", vec![]);
        d.after = vec![format!("{PROCESS_TASK_PREFIX}p@started")];

        let (tasks, _tmp) = build_test_tasks(
            vec![p, d],
            vec![
                format!("{PROCESS_TASK_PREFIX}p"),
                format!("{PROCESS_TASK_PREFIX}d"),
            ],
            false,
        )
        .await;

        let p_idx = tasks.task_index_by_name[&format!("{PROCESS_TASK_PREFIX}p")];
        let p_cfg = tasks.graph[p_idx]
            .read()
            .await
            .build_process_config(&tasks.env, &tasks.bash, tasks.supervisor)
            .unwrap();
        tasks
            .process_manager
            .start_command(&p_cfg, None)
            .await
            .unwrap();

        wait_phase(&tasks, "p", ProcessPhase::Exited).await;

        assert!(
            !tasks.dependency_parked("d").await,
            "an exited process satisfies @started, so d must not be parked"
        );

        tasks.process_manager().stop_and_keep("p").await.unwrap();
        assert_eq!(
            tasks.process_manager().get_phase("p").await,
            Some(ProcessPhase::Stopped),
        );

        assert!(
            !tasks.dependency_parked("d").await,
            "a process that started then exited still satisfies @started after \
             an explicit stop; d must not be judged dependency-parked"
        );

        tasks.process_manager().stop_all().await.unwrap();
    }

    #[tokio::test]
    async fn ordinary_task_run_does_not_register_unscheduled_processes() {
        let enter_shell = "devenv:enterShell";
        let docs = format!("{PROCESS_TASK_PREFIX}docs");
        let (tasks, _tmp) = build_test_tasks(
            vec![
                oneshot_task(enter_shell, vec![]),
                process_task(&docs, vec![]),
            ],
            vec![enter_shell.to_string()],
            false,
        )
        .await;

        tasks.run(false).await;

        assert_eq!(
            tasks.process_manager().get_phase("docs").await,
            None,
            "an unscheduled process must not leak into a transient task runner"
        );
    }

    #[tokio::test]
    async fn subset_cold_start_keeps_unrelated_processes_known() {
        let api = format!("{PROCESS_TASK_PREFIX}api");
        let db = format!("{PROCESS_TASK_PREFIX}db");
        let worker = format!("{PROCESS_TASK_PREFIX}worker");
        let blocked = format!("{PROCESS_TASK_PREFIX}blocked");

        let (tasks, _tmp) = build_test_tasks_with_run_mode(
            vec![
                process_task(&api, vec![&format!("{db}@started")]),
                process_task(&db, vec![]),
                process_task(&worker, vec![&format!("{blocked}@started")]),
                process_task(&blocked, vec![]),
            ],
            vec![api.clone()],
            RunMode::Before,
            false,
        )
        .await;

        let scheduled = task_names(&tasks).await;
        assert!(scheduled.contains(&api), "requested process must run");
        assert!(scheduled.contains(&db), "its dependency must run");
        assert!(
            !scheduled.contains(&worker),
            "an unrelated process must not run on a subset start"
        );
        assert!(
            !scheduled.contains(&blocked),
            "an unrelated dependency must not run on a subset start"
        );

        for name in [&api, &db, &worker, &blocked] {
            assert!(
                tasks.task_index_by_name.contains_key(name),
                "{name} must remain addressable after a subset cold start"
            );
        }

        tasks.run(true).await;
        for name in ["worker", "blocked"] {
            assert_eq!(
                tasks.process_manager().get_phase(name).await,
                Some(ProcessPhase::NotStarted),
                "{name} must be visible to the retained manager without launching"
            );
        }

        tasks.process_manager().stop_all().await.unwrap();
    }

    #[tokio::test]
    async fn ignore_process_deps_prunes_non_root_processes() {
        // Graph: root process A depends on process B (non-root)
        // With ignore_process_deps=true, B should be pruned from the subgraph
        let (tasks, _tmp) = build_test_tasks(
            vec![
                process_task("ns:proc:a", vec!["ns:proc:b@completed"]),
                process_task("ns:proc:b", vec![]),
            ],
            vec!["ns:proc:a".to_string()],
            true,
        )
        .await;

        // Only root process A should remain
        let names = task_names(&tasks).await;
        assert_eq!(names, vec!["ns:proc:a"]);
    }

    #[tokio::test]
    async fn ignore_process_deps_keeps_oneshot_deps() {
        // Graph: root process A depends on oneshot B (migration)
        // With ignore_process_deps=true, B should NOT be pruned
        let (tasks, _tmp) = build_test_tasks(
            vec![
                process_task("ns:proc:a", vec!["ns:task:migrate"]),
                oneshot_task("ns:task:migrate", vec![]),
            ],
            vec!["ns:proc:a".to_string()],
            true,
        )
        .await;

        // Both should remain: root process A and oneshot migration
        let names = task_names(&tasks).await;
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"ns:proc:a".to_string()));
        assert!(names.contains(&"ns:task:migrate".to_string()));
    }

    #[tokio::test]
    async fn ignore_process_deps_false_keeps_all() {
        // Same graph but with ignore_process_deps=false: both should remain
        let (tasks, _tmp) = build_test_tasks(
            vec![
                process_task("ns:proc:a", vec!["ns:proc:b@completed"]),
                process_task("ns:proc:b", vec![]),
            ],
            vec!["ns:proc:a".to_string()],
            false,
        )
        .await;

        assert_eq!(tasks.tasks_order.len(), 2);
    }

    #[tokio::test]
    async fn ignore_process_deps_keeps_root_processes() {
        // Both A and B are roots — neither should be pruned even with ignore_process_deps
        let (tasks, _tmp) = build_test_tasks(
            vec![
                process_task("ns:proc:a", vec![]),
                process_task("ns:proc:b", vec![]),
            ],
            vec!["ns:proc:a".to_string(), "ns:proc:b".to_string()],
            true,
        )
        .await;

        assert_eq!(tasks.tasks_order.len(), 2);
    }

    #[tokio::test]
    async fn ignore_process_deps_preserves_transitive_oneshot() {
        // Graph: root process A -> process B (non-root) -> oneshot C
        // B gets pruned, but C should still be in the subgraph
        let (tasks, _tmp) = build_test_tasks(
            vec![
                process_task("ns:proc:a", vec!["ns:proc:b@completed"]),
                process_task("ns:proc:b", vec!["ns:task:setup"]),
                oneshot_task("ns:task:setup", vec![]),
            ],
            vec!["ns:proc:a".to_string()],
            true,
        )
        .await;

        let names = task_names(&tasks).await;
        assert!(names.contains(&"ns:proc:a".to_string()));
        assert!(names.contains(&"ns:task:setup".to_string()));
        assert!(!names.contains(&"ns:proc:b".to_string()));
        assert_eq!(names.len(), 2);
    }
}

#[cfg(test)]
mod hierarchy_tests {
    use super::*;
    use petgraph::graph::DiGraph;

    /// Helper to create a simple graph and compute hierarchy edges.
    /// Returns (edges, task_ids) where task_ids maps node indices to their IDs.
    fn setup_test(
        nodes: usize,
        graph_edges: &[(usize, usize)],
        roots: &[usize],
        tasks_order: &[usize],
    ) -> (Vec<(u64, u64)>, HashMap<NodeIndex, u64>) {
        let mut graph: DiGraph<&str, ()> = DiGraph::new();
        let node_indices: Vec<_> = (0..nodes).map(|_| graph.add_node("task")).collect();

        for &(from, to) in graph_edges {
            // Edge from dependency to dependent (from is dependency of to)
            graph.add_edge(node_indices[from], node_indices[to], ());
        }

        let roots_vec: Vec<_> = roots.iter().map(|&i| node_indices[i]).collect();
        let order: Vec<_> = tasks_order.iter().map(|&i| node_indices[i]).collect();
        let task_ids: HashMap<_, _> = node_indices
            .iter()
            .enumerate()
            .map(|(i, &idx)| (idx, (i + 1) as u64))
            .collect();

        let orchestration_id = 100;
        let edges =
            compute_hierarchy_edges(&graph, &order, &roots_vec, &task_ids, orchestration_id);
        (edges, task_ids)
    }

    #[test]
    fn test_single_root_task() {
        // Single root task should appear under orchestration
        let (edges, _) = setup_test(1, &[], &[0], &[0]);
        assert_eq!(edges, vec![(100, 1)]); // orchestration -> task1
    }

    #[test]
    fn test_linear_chain() {
        // Linear chain: task0 -> task1 -> task2 (task0 is dependency of task1, etc.)
        // task2 is root, task1 depends on task0
        // Expected hierarchy:
        //   orchestration -> task2
        //   task2 -> task1
        //   task1 -> task0
        let (edges, _) = setup_test(
            3,
            &[(0, 1), (1, 2)], // task0 <- task1 <- task2
            &[2],              // task2 is root
            &[0, 1, 2],        // topological order
        );

        assert!(edges.contains(&(100, 3))); // orchestration -> task2 (id=3)
        assert!(edges.contains(&(3, 2))); // task2 -> task1
        assert!(edges.contains(&(2, 1))); // task1 -> task0
        assert_eq!(edges.len(), 3);
    }

    #[test]
    fn test_diamond_dependency() {
        // Diamond pattern:
        //     task3 (root)
        //    /    \
        // task1   task2
        //    \    /
        //     task0 (shared dependency)
        //
        // task0 should appear under BOTH task1 and task2
        let (edges, _) = setup_test(
            4,
            &[
                (0, 1), // task0 <- task1
                (0, 2), // task0 <- task2
                (1, 3), // task1 <- task3
                (2, 3), // task2 <- task3
            ],
            &[3],          // task3 is root
            &[0, 1, 2, 3], // topological order
        );

        // task3 under orchestration
        assert!(edges.contains(&(100, 4))); // orchestration -> task3 (id=4)
        // task1, task2 under task3
        assert!(edges.contains(&(4, 2))); // task3 -> task1
        assert!(edges.contains(&(4, 3))); // task3 -> task2
        // task0 under both task1 and task2 (diamond)
        assert!(edges.contains(&(2, 1))); // task1 -> task0
        assert!(edges.contains(&(3, 1))); // task2 -> task0
        assert_eq!(edges.len(), 5);
    }

    #[test]
    fn test_transitive_dependency_not_duplicated() {
        // Chain where D1 depends on D2 which depends on task0
        //   task2 (root)
        //     |
        //   task1
        //     |
        //   task0
        //
        // task0 should only appear under task1, not task2
        // (task2 reaches task0 through task1, so task1 "covers" the path)
        let (edges, _) = setup_test(3, &[(0, 1), (1, 2)], &[2], &[0, 1, 2]);

        // task0 should only appear under task1, not task2
        assert!(edges.contains(&(2, 1))); // task1 -> task0
        assert!(!edges.iter().any(|&(p, c)| p == 3 && c == 1)); // task2 should NOT have edge to task0
    }

    #[test]
    fn test_multiple_roots() {
        // Two independent roots
        // task0 -> task1 (root)
        // task2 -> task3 (root)
        let (edges, _) = setup_test(4, &[(0, 1), (2, 3)], &[1, 3], &[0, 2, 1, 3]);

        // Both roots under orchestration
        assert!(edges.contains(&(100, 2))); // orchestration -> task1
        assert!(edges.contains(&(100, 4))); // orchestration -> task3
        // Dependencies under their roots
        assert!(edges.contains(&(2, 1))); // task1 -> task0
        assert!(edges.contains(&(4, 3))); // task3 -> task2
        assert_eq!(edges.len(), 4);
    }

    #[test]
    fn test_task_with_no_dependents_falls_back() {
        // A non-root task with no dependents in the task order
        // This can happen if the dependent is filtered out
        // task0 has no outgoing edges in the filtered graph
        let (edges, _) = setup_test(
            2,
            &[],     // no edges
            &[1],    // only task1 is root
            &[0, 1], // task0 is not a root but has no dependents
        );

        // Both should be under orchestration
        assert!(edges.contains(&(100, 1))); // orchestration -> task0
        assert!(edges.contains(&(100, 2))); // orchestration -> task1
        assert_eq!(edges.len(), 2);
    }

    #[test]
    fn test_complex_dag() {
        // More complex DAG:
        //       task4 (root)
        //      /  |  \
        //   task1 task2 task3
        //      \  |  /
        //       task0
        //
        // task0 should appear under all three middle tasks
        let (edges, _) = setup_test(
            5,
            &[
                (0, 1), // task0 <- task1
                (0, 2), // task0 <- task2
                (0, 3), // task0 <- task3
                (1, 4), // task1 <- task4
                (2, 4), // task2 <- task4
                (3, 4), // task3 <- task4
            ],
            &[4],
            &[0, 1, 2, 3, 4],
        );

        // Root under orchestration
        assert!(edges.contains(&(100, 5))); // orchestration -> task4
        // Middle layer under root
        assert!(edges.contains(&(5, 2))); // task4 -> task1
        assert!(edges.contains(&(5, 3))); // task4 -> task2
        assert!(edges.contains(&(5, 4))); // task4 -> task3
        // task0 under all three middle tasks
        assert!(edges.contains(&(2, 1))); // task1 -> task0
        assert!(edges.contains(&(3, 1))); // task2 -> task0
        assert!(edges.contains(&(4, 1))); // task3 -> task0
        assert_eq!(edges.len(), 7);
    }
}
