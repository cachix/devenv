use crate::config::{Config, RunMode, parse_dependency};
use crate::error::Error;
use crate::task_cache::TaskCache;
use crate::task_state::TaskState;
use crate::types::{
    DepSatisfaction, DependencyKind, OneshotStatus, Output, Outputs, PROCESS_TASK_PREFIX, Skipped,
    TaskCompleted, TaskFailure, TaskStatus, TaskType, TasksStatus, VerbosityLevel,
};
use devenv_activity::{Activity, ActivityInstrument, TaskInfo, emit_task_hierarchy, next_id};
use devenv_processes::{NativeProcessManager, ProcessConfig, ProcessPhase, StartOutcome};
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
            ignore_process_deps: self.config.ignore_process_deps,
            task_index_by_name: HashMap::new(),
            start_with_deps_lock: Mutex::new(()),
        };

        tasks.resolve_dependencies(task_indices).await?;
        tasks.tasks_order = tasks.schedule().await?;
        // schedule() narrows what runs (`tasks_order`) but keeps the full graph,
        // so this lookup covers every configured task. start_with_deps relies on
        // that: a process this run did not bring up can still be found and
        // scheduled later instead of being rejected as unknown.
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
    /// Full task name -> graph node index. Covers every configured task:
    /// `schedule()` narrows what runs (`tasks_order`) but keeps the full graph,
    /// so a process not brought up by this run is still addressable here and can
    /// be scheduled later by `start_with_deps`.
    pub(crate) task_index_by_name: HashMap<String, NodeIndex>,
    /// Serializes `start_with_deps` calls. Two concurrent `up`s for the same
    /// stopped process would otherwise both observe the pre-re-arm phase and
    /// spawn duplicate dependency waiters whose launch-race loser records a
    /// false `Failed` launch outcome in the graph node.
    pub(crate) start_with_deps_lock: Mutex<()>,
}

/// One dependency's evaluation, produced by `Tasks::eval_dep` and shared
/// between the dependency waiter and the parked judgment so the dependency
/// waiter and the manager's `Wait` settled rule can never diverge.
struct DepEval {
    /// Full task name of the dependency (e.g. `devenv:processes:db`).
    task_name: String,
    sat: DepSatisfaction,
    /// Live manager phase for process-type dependencies (`None` for oneshot
    /// dependencies and for process dependencies without a manager entry).
    live_phase: Option<ProcessPhase>,
    /// True when this dependency is itself actively in flight: a oneshot whose
    /// command is currently running. Such a dependency is progressing on its
    /// own, so a process waiting on it is never dependency-parked — its
    /// (possibly since-regressed) dependencies are irrelevant now that it has
    /// launched. Always false for process dependencies, whose progress is read
    /// from `live_phase`.
    dep_in_flight: bool,
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
                        Some(ProcessPhase::Exited) => status.succeeded += 1,
                        Some(ProcessPhase::GaveUp) => status.failed += 1,
                        Some(
                            ProcessPhase::Waiting | ProcessPhase::Starting | ProcessPhase::Ready,
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
            }
        }

        // When ignore_process_deps is set, remove non-root process-type tasks
        // from the visited set. This prevents process duplication when process-compose
        // manages process ordering via depends_on.
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

        // The subgraph exists only to order the scheduled set. Keep the full
        // graph as `self.graph` so every configured task stays addressable and
        // `task_index_by_name` (built right after) covers all of them; a later
        // `start_with_deps` can then find and schedule a process this run did
        // not bring up instead of rejecting it as unknown. `self.roots` keeps
        // its full-graph indices for the same reason (no remap).
        let full_by_sub: HashMap<NodeIndex, NodeIndex> =
            node_map.iter().map(|(&full, &sub)| (sub, full)).collect();

        // Topologically sort the scheduled subgraph, then map the order back
        // onto the retained full graph.
        match toposort(&subgraph, None) {
            Ok(order) => Ok(order.into_iter().map(|sub| full_by_sub[&sub]).collect()),
            Err(cycle) => Err(Error::CycleDetected(
                subgraph[cycle.node_id()].read().await.task.name.clone(),
            )),
        }
    }

    #[instrument(skip(self))]
    pub async fn run(&self, is_process_mode: bool) -> Outputs {
        // Create an orchestration-level Operation activity to track overall progress
        // Using Operation (not Task) so it doesn't count in the task summary
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

        self.run_internal(orchestration_activity).await
    }

    /// Run with a caller-provided parent activity instead of creating a new top-level one.
    /// Used by `up()` Phase 4 to nest process execution under "Running processes".
    #[instrument(skip(self, parent_activity))]
    pub async fn run_with_parent_activity(&self, parent_activity: Arc<Activity>) -> Outputs {
        self.run_internal(parent_activity).await
    }

    /// Start a subset of already-built process tasks, honouring their
    /// `after`/`before` dependencies via the same engine as the initial run.
    ///
    /// Used when a long-lived manager is already running (e.g. a daemon started
    /// by `devenv up -d` or `devenv shell`) and a later `devenv up [names]`
    /// wants to bring up more processes. Rather than the CLI re-deriving the
    /// dependency order and force-launching each process over the control
    /// socket, the daemon drives them through `wait_for_task_deps` +
    /// `run_process` — so out-of-subset and already-running dependencies are
    /// resolved against the live task graph exactly like the cold-start path.
    ///
    /// `names` are process names without the `devenv:processes:` prefix. An
    /// empty `names` is a no-op. Every requested name is classified into
    /// exactly one [`StartOutcome`] bucket:
    /// - `scheduled`: re-armed `Waiting` and handed to the dependency-driven
    ///   launch path;
    /// - `skipped`: already running, starting, or pending on a dependency —
    ///   left untouched (already-running ones count as satisfied
    ///   dependencies);
    /// - `unknown`: not present in this scheduler's task graph (the manager
    ///   was started with a different configuration or a subset);
    /// - `failed`: known but could not be scheduled (e.g. building the
    ///   process config failed).
    ///
    /// Returns once the processes have been *scheduled*, not once they are
    /// ready: each one launches in a detached task that waits for its
    /// dependencies in the background. A process whose dependency is never
    /// satisfied simply stays `Waiting` in the manager (visible in the TUI),
    /// rather than blocking the caller — so an attaching `devenv up` never
    /// hangs on a dependent it cannot complete.
    pub async fn start_with_deps(&self, names: &[String]) -> StartOutcome {
        // Serialized: concurrent calls for the same stopped name would both
        // read the pre-re-arm phase and spawn duplicate dependency waiters
        // (see the field doc on `start_with_deps_lock`). The loop never
        // awaits long work — launches are detached — so the hold is short.
        let _serialize = self.start_with_deps_lock.lock().await;
        let mut outcome = StartOutcome::default();

        // Dedup while preserving order: a name requested twice (e.g. `devenv up
        // foo foo`) would otherwise be re-armed on the first pass (scheduled)
        // and seen Waiting on the second (skipped), landing in two buckets.
        let mut seen = std::collections::HashSet::new();
        let names: Vec<&String> = names.iter().filter(|n| seen.insert(n.as_str())).collect();

        for name in names {
            let task_name = format!("{PROCESS_TASK_PREFIX}{name}");
            let Some(&index) = self.task_index_by_name.get(&task_name) else {
                tracing::debug!(process = %name, "up requested for a process not in the task graph");
                outcome.unknown.push(name.clone());
                continue;
            };

            match self.process_manager.get_phase(name).await {
                // Already running, or already scheduled and waiting on a
                // dependency: a live dep-waiter (or the process itself) is in
                // flight, so leave it. Re-arming a `Waiting` process would spawn
                // a second waiter that later errors when it loses the launch
                // race; already-running ones count as satisfied dependencies.
                // A mid-launch `Launching` entry reports `Starting`, so an
                // in-flight launch is skipped here too.
                Some(ProcessPhase::Starting | ProcessPhase::Ready | ProcessPhase::Waiting) => {
                    outcome.skipped.push(name.clone());
                    continue;
                }
                // Exited or gave up on its own: the manager entry is still
                // `Active` (only an explicit stop produces `Stopped`), and
                // `rearm_waiting` refuses to touch an `Active` entry. Normalize
                // it to `Stopped` first — `stop_and_keep` aborts the dead
                // supervisor and tailers, releases its ports, and keeps the TUI
                // row — so the re-arm + launch path below can relaunch it.
                Some(ProcessPhase::Exited | ProcessPhase::GaveUp) => {
                    if let Err(e) = self.process_manager.stop_and_keep(name).await {
                        tracing::warn!(process = %name, "failed to reset exited process before relaunch: {e}");
                    }
                }
                _ => {}
            }

            // Names arriving here were explicitly chosen (by the user or by
            // the client's up-enabled default set, see
            // `Devenv::resolve_launch_processes`); an explicitly requested
            // process always starts, so force `start.enable` on for the
            // launch — `run_process`'s `launch_waiting` then launches it even
            // if it was registered auto-start-off (e.g. a non-shell process
            // in a `devenv shell` daemon).
            let config = {
                let ts = self.graph[index].read().await;
                match ts.build_process_config(&self.env, &self.bash) {
                    Ok(mut config) => {
                        config.start.enable = true;
                        config
                    }
                    Err(e) => {
                        tracing::error!(process = %name, "failed to build process config: {e}");
                        outcome.failed.push(name.clone());
                        continue;
                    }
                }
            };
            // The re-armed Waiting entry holds the config `launch_waiting` will
            // launch; `run_process` (below) only reads name/probe off its copy.
            self.process_manager.rearm_waiting(config.clone()).await;
            // Clear a stale launch outcome so dependents fall back to the
            // manager's Waiting phase rather than a dead Completed status.
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

            // Detached: wait for deps and launch in the background so a
            // never-satisfiable dependency leaves this process `Waiting` instead
            // of blocking the `Start` reply. Mirrors how the cold-start path spawns
            // per-process dependency checkers.
            tokio::spawn(async move {
                let (dep_cancelled, dep_failed) =
                    Self::wait_for_task_deps(&deps, &process_manager, &notify_finished, &shutdown)
                        .await;
                if dep_cancelled || dep_failed {
                    process_manager.cancel_waiting(&process_name).await;
                    return;
                }

                // The read guard must drop before the failure path takes the
                // write lock below; an `if let` scrutinee guard lives through
                // the then-block and would self-deadlock.
                let launch_result = {
                    let ts = task_state.read().await;
                    ts.run_process(&process_manager, config).await
                };
                if let Err(e) = launch_result {
                    tracing::error!(process = %process_name, "failed to start process: {e}");
                    // Launch outcome is graph-owned: record it so dependents
                    // see NeverSatisfiable while the manager has a Stopped entry.
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
                // Success or auto-start-off: the manager owns the phase from here.
            });
        }

        outcome
    }

    /// Increment the completed counter, update the orchestration progress bar, and
    /// notify the dependency and UI loops. Every task must call this exactly once
    /// when its work (success, failure, skip) is done.
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

    /// Mark a task as cancelled or dependency_failed, update progress, and notify.
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

    /// Evaluate one dependency edge. Shared verbatim between the dependency
    /// waiter loop (`wait_for_task_deps`) and the parked judgment
    /// (`dependency_parked`) so the two can never diverge.
    ///
    /// Process dependencies are evaluated against the manager's live phase;
    /// the graph node is only consulted for graph-owned launch outcomes.
    /// The dep's read guard is held only for the duration of this call.
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
            // The dependency phase, not the displayed phase: a process the user
            // explicitly stopped after it had exited still satisfies
            // `<proc>@started` (it did run), so a dependent is not stranded.
            let live_phase = process_manager.get_dependency_phase(pname).await;
            let sat = match live_phase {
                // A live manager phase wins: it cannot go stale and a
                // re-armed Waiting entry outranks a dead Completed node.
                Some(
                    phase @ (ProcessPhase::Waiting
                    | ProcessPhase::Starting
                    | ProcessPhase::Ready
                    | ProcessPhase::Exited
                    | ProcessPhase::GaveUp),
                ) => crate::types::is_process_dep_satisfied(phase, dep_kind),
                // No entry, NotStarted, or Stopped: a terminal node
                // status is the graph-owned launch outcome (launch
                // failure, dependency failure, cancellation) and is
                // conclusive; otherwise fall back to the phase itself
                // so e.g. an auto-start-off dep satisfies @completed.
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

    /// Whether the named `Waiting` process is dependency-parked: it has at
    /// least one unsatisfied dependency, and every unsatisfied dependency is
    /// blocked on external action — a stopped or not-started process, or
    /// transitively another parked `Waiting` process. Judged live against the
    /// task graph and the manager's phases at call time (consulted by the
    /// manager's `Wait` settled rule via [`devenv_processes::ProcessScheduler`]),
    /// so it can never act on stale state.
    ///
    /// Anything else still in flight (a starting/launching dependency, a
    /// running oneshot, a missing manager entry) counts as progressing —
    /// conservative, so `Wait` keeps blocking. A process with no unsatisfied
    /// dependencies is not parked (it is about to launch), and unknown names
    /// are not parked.
    pub async fn dependency_parked(&self, process_name: &str) -> bool {
        let task_name = format!("{PROCESS_TASK_PREFIX}{process_name}");
        self.task_dependency_parked(&task_name).await
    }

    /// Recursive core of [`Self::dependency_parked`], keyed by full task
    /// name. The graph is an acyclic toposorted DAG (cycles are rejected at
    /// build time), so recursing over `Waiting` dependencies terminates.
    /// Boxed because async recursion needs a sized future. `eval_dep` drops
    /// the dependency's read guard before any recursion below, so no node
    /// lock is held across a relock (Edition 2024 scrutinee rule).
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
                // NeverSatisfiable deps are not blockers: external action is
                // already required either way, and the dependency waiter will
                // conclude (cancel_waiting) once its evaluation reaches them.
                if eval.sat != DepSatisfaction::NotYet {
                    continue;
                }
                any_blocker = true;
                let parked = match eval.live_phase {
                    Some(ProcessPhase::NotStarted | ProcessPhase::Stopped) => true,
                    Some(ProcessPhase::Waiting) => {
                        self.task_dependency_parked(&eval.task_name).await
                    }
                    // Starting/Ready processes are genuinely in flight.
                    Some(_) => false,
                    // A currently-running oneshot is progressing on its own —
                    // it has already launched, so whatever it depended on (even
                    // a since-stopped process) no longer blocks it. Not parked.
                    None if eval.dep_in_flight => false,
                    // A oneshot that has NOT launched yet (or any non-process
                    // dep) is parked iff its own unsatisfied dependencies are
                    // all parked: recurse so a process blocked transitively
                    // through a oneshot that itself waits on a
                    // stopped/not-started process is still judged parked.
                    None => self.task_dependency_parked(&eval.task_name).await,
                };
                if !parked {
                    return false;
                }
            }
            any_blocker
        })
    }

    /// Wait for task dependencies to be satisfied in the background.
    /// Each dependency is evaluated via [`Self::eval_dep`].
    /// Returns `(cancelled, dependency_failed)`.
    async fn wait_for_task_deps(
        deps: &[(Arc<RwLock<TaskState>>, DependencyKind)],
        process_manager: &Arc<NativeProcessManager>,
        notify_finished: &Notify,
        shutdown: &tokio_shutdown::Shutdown,
    ) -> (bool, bool) {
        loop {
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
                match Self::eval_dep(dep_state, dep_kind, process_manager)
                    .await
                    .sat
                {
                    DepSatisfaction::Satisfied => {}
                    DepSatisfaction::NeverSatisfiable => return (false, true),
                    DepSatisfaction::NotYet => {
                        all_satisfied = false;
                        break;
                    }
                }
            }

            if all_satisfied {
                return (false, false);
            }

            tokio::select! {
                _ = notified => {},
                _ = shutdown.wait_for_shutdown() => {
                    return (true, false);
                }
            }
        }
    }

    async fn run_internal(&self, orchestration_activity: Arc<Activity>) -> Outputs {
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

        let outputs = Arc::new(Mutex::new(Outputs::new()));
        let mut running_tasks = self.shutdown.join_set();

        // Pre-register all process tasks with the process manager so they
        // appear in the TUI immediately, regardless of topological sort order.
        let mut process_configs: HashMap<NodeIndex, ProcessConfig> = HashMap::new();
        for &index in &self.tasks_order {
            if self.shutdown.is_cancelled() {
                break;
            }
            let ts = self.graph[index].read().await;
            if ts.task.r#type != TaskType::Process || ts.task.command.is_none() {
                continue;
            }
            match ts.build_process_config(&self.env, &self.bash) {
                Ok(config) => {
                    self.process_manager
                        .register_waiting(config.clone(), Some(orchestration_activity.id()))
                        .await;

                    process_configs.insert(index, config);
                }
                Err(e) => {
                    let name = ts.task.name.clone();
                    drop(ts);
                    let mut ts = self.graph[index].write().await;
                    error!("Failed to build process config for {}: {}", name, e);
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

        // Start the API server so `devenv processes wait` can connect.
        // This must happen after pre-registration so the socket is available
        // when external clients connect. Only start if there are process tasks.
        if !process_configs.is_empty()
            && let Err(e) = self.process_manager.start_api_server()
        {
            error!("Failed to start process manager API server: {}", e);
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

                let deps = self.collect_deps(*index);

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
                            "Process task {}: waiting for {} dependencies",
                            config.name,
                            deps.len()
                        );
                        let (dep_cancelled, dep_failed) = Self::wait_for_task_deps(
                            &deps,
                            &process_manager_clone,
                            &notify_finished_clone,
                            &shutdown_clone,
                        )
                        .await;
                        tracing::debug!(
                            "Process task {}: deps done, cancelled={}, failed={}",
                            config.name,
                            dep_cancelled,
                            dep_failed
                        );

                        if dep_cancelled || dep_failed {
                            // Clean up the Waiting entry in the process manager
                            // so the TUI no longer shows this process as "Waiting".
                            process_manager_clone.cancel_waiting(&config.name).await;

                            Self::mark_task_skipped(
                                &task_state_clone,
                                task_activity_id,
                                dep_cancelled,
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
                                    "Failed to start process task {}: {}",
                                    task_state.task.name, e
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
            let deps = self.collect_deps(*index);

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
                    // Wait for dependencies in background
                    let (dep_cancelled, dep_failed) = Self::wait_for_task_deps(
                        &deps,
                        &process_manager_clone,
                        &notify_finished_clone,
                        &shutdown_clone,
                    )
                    .await;

                    if dep_cancelled || dep_failed {
                        Self::mark_task_skipped(
                            &task_state_clone,
                            task_activity_id,
                            dep_cancelled,
                            &completed_tasks_clone,
                            total_tasks,
                            &orchestration_activity_inner,
                            &notify_finished_clone,
                            &notify_ui_clone,
                        )
                        .await;
                        return;
                    }

                    // Reset the timer
                    let now = Instant::now();

                    {
                        let mut task_state = task_state_clone.write().await;
                        task_state.status = TaskStatus::Oneshot(OneshotStatus::Running(now));
                    };

                    // Notify UI that task is starting
                    notify_ui_clone.notify_one();
                    let completed = {
                        let outputs = outputs_clone.lock().await.clone();
                        match task_state_clone
                            .read()
                            .await
                            .run(
                                now,
                                &outputs,
                                &cache,
                                shutdown_clone.cancellation_token(),
                                task_activity_id,
                                refresh_task_cache,
                                &shell_env,
                            )
                            .await
                        {
                            Ok(result) => result,
                            Err(e) => {
                                error!("Task failed with error: {}", e);
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
                        let mut task_state = task_state_clone.write().await;
                        match &completed {
                            TaskCompleted::Success(_, Output(Some(output))) => {
                                outputs_clone
                                    .lock()
                                    .await
                                    // TODO: remove clone
                                    .insert(task_state.task.name.clone(), output.clone());

                                // Store the task output for all tasks to support future reuse
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
                                            "Failed to store task output for {}: {}",
                                            task_name,
                                            e
                                        );
                                    }
                                }
                            }
                            TaskCompleted::Skipped(Skipped::Cached(Output(Some(output)))) => {
                                outputs_clone
                                    .lock()
                                    .await
                                    // TODO: fix clone
                                    .insert(task_state.task.name.clone(), output.clone());

                                // Store task output if we're having status or exec_if_modified
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
                                            "Failed to store task output for {}: {}",
                                            task_name,
                                            e
                                        );
                                    }
                                }
                            }
                            _ => {}
                        }

                        task_state.status = TaskStatus::Completed(completed);
                    }

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

        Arc::try_unwrap(outputs).unwrap().into_inner()
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
        self.start_with_deps(&names).await
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

    /// Helper to build a minimal Tasks struct for testing schedule().
    /// Returns the TempDir alongside Tasks to keep the directory alive for the test.
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

    fn disabled_process_task(name: &str, after: Vec<&str>) -> TaskConfig {
        TaskConfig {
            process: Some(devenv_processes::ProcessConfig {
                start: devenv_processes::config::StartConfig { enable: false },
                ..Default::default()
            }),
            ..process_task(name, after)
        }
    }

    /// Collect task names from the scheduled tasks_order.
    async fn task_names(tasks: &Tasks) -> Vec<String> {
        let mut names = Vec::new();
        for idx in &tasks.tasks_order {
            names.push(tasks.graph[*idx].read().await.task.name.clone());
        }
        names
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

    /// A long-running process task.
    fn long_process_task(name: &str, after: Vec<&str>) -> TaskConfig {
        process_task_with_command(name, after, "sleep 100")
    }

    /// A process task that exits successfully on its own and is not restarted
    /// (the default restart policy is `OnFailure`, and `echo` exits 0). Used to
    /// drive a process to the `Exited` phase in tests.
    fn self_exit_process_task(name: &str, after: Vec<&str>) -> TaskConfig {
        process_task_with_command(name, after, "echo")
    }

    /// Event-driven phase wait: wakes on `notify_finished` (the manager fires
    /// it on every lifecycle and supervisor transition) and re-reads the
    /// manager phase. The timeout is a failure bound, never a poll interval.
    /// A missing manager entry keeps waiting until the failure bound: entries
    /// must never vanish mid-lifecycle.
    async fn wait_phase(tasks: &Tasks, name: &str, want: ProcessPhase) {
        tokio::time::timeout(std::time::Duration::from_secs(60), async {
            loop {
                let notified = tasks.notify_finished.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if tasks.process_manager.get_phase(name).await == Some(want) {
                    return;
                }
                notified.await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for process {name} to reach {want:?}"))
    }

    #[tokio::test]
    async fn start_with_deps_relaunches_stopped_process() {
        // A single up-enabled long-running process, started by the cold run,
        // then stopped, then brought back by start_with_deps (the attach path).
        let (tasks, _tmp) = build_test_tasks(
            vec![long_process_task("web", vec![])],
            vec![format!("{PROCESS_TASK_PREFIX}web")],
            false,
        )
        .await;

        let parent = Arc::new(devenv_activity::start!(
            devenv_activity::Activity::operation("test").parent(None)
        ));
        let _ = tasks.run_with_parent_activity(parent).await;

        // The process should be running after the cold start.
        wait_phase(&tasks, "web", ProcessPhase::Ready).await;

        tasks.process_manager.stop_and_keep("web").await.unwrap();
        assert_eq!(
            tasks.process_manager.get_phase("web").await,
            Some(ProcessPhase::Stopped)
        );

        // Attach path: bring the stopped process back up.
        let outcome = tasks.start_with_deps(&["web".to_string()]).await;
        assert_eq!(outcome.scheduled, vec!["web".to_string()]);
        // start_with_deps must relaunch the stopped process.
        wait_phase(&tasks, "web", ProcessPhase::Ready).await;

        let _ = tasks.process_manager.stop_all().await;
    }

    #[tokio::test]
    async fn start_with_deps_relaunches_exited_process() {
        // Regression: a process that exits on its own stays `Active` with phase
        // `Exited` (only an explicit stop produces `Stopped`), and the manager
        // refuses to re-arm an `Active` entry. The attach path must normalize it
        // back to a launchable state, rather than no-op'ing the re-arm and then
        // bailing in `launch_waiting`.
        //
        // The process exits cleanly on its first run but sleeps on the second,
        // so reaching `Ready` after `start_with_deps` proves it was actually
        // relaunched (not merely left in its original `Exited` state).
        let marker_dir = tempfile::tempdir().unwrap();
        let marker = marker_dir.path().join("ran");
        let exec = format!(
            "if [ -e '{m}' ]; then sleep 100; else : > '{m}'; fi",
            m = marker.display()
        );
        let task = TaskConfig {
            name: format!("{PROCESS_TASK_PREFIX}web"),
            r#type: TaskType::Process,
            command: Some(exec),
            ..Default::default()
        };

        let (tasks, _tmp) =
            build_test_tasks(vec![task], vec![format!("{PROCESS_TASK_PREFIX}web")], false).await;

        let parent = Arc::new(devenv_activity::start!(
            devenv_activity::Activity::operation("test").parent(None)
        ));
        let _ = tasks.run_with_parent_activity(parent).await;

        // First run exits cleanly -> entry stays Active with phase Exited.
        wait_phase(&tasks, "web", ProcessPhase::Exited).await;

        // Attach path: relaunch the self-exited process. The second run sleeps,
        // so reaching Ready proves start_with_deps actually relaunched it.
        let outcome = tasks.start_with_deps(&["web".to_string()]).await;
        assert_eq!(outcome.scheduled, vec!["web".to_string()]);
        wait_phase(&tasks, "web", ProcessPhase::Ready).await;

        let _ = tasks.process_manager.stop_all().await;
    }

    #[tokio::test]
    async fn start_with_deps_waits_for_unsatisfied_dependency() {
        // beta after gamma@started; gamma is stopped. Attaching `up beta` must
        // NOT launch beta (its dependency is unmet) and must not hang.
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

        let parent = Arc::new(devenv_activity::start!(
            devenv_activity::Activity::operation("test").parent(None)
        ));
        let _ = tasks.run_with_parent_activity(parent).await;
        wait_phase(&tasks, "beta", ProcessPhase::Ready).await;

        // Stop both; gamma is now an unmet dependency for beta.
        tasks.process_manager.stop_and_keep("beta").await.unwrap();
        tasks.process_manager.stop_and_keep("gamma").await.unwrap();

        // Attach `up beta`: beta must stay waiting (gamma is stopped).
        let outcome = tasks.start_with_deps(&["beta".to_string()]).await;
        assert_eq!(outcome.scheduled, vec!["beta".to_string()]);

        // Drive the detached dep waiter on this current-thread test runtime:
        // wake it explicitly and yield so it evaluates its dependencies. Yields
        // hand control to ready tasks; no wall-clock timing involved.
        for _ in 0..64 {
            tasks.notify_finished.notify_waiters();
            tokio::task::yield_now().await;
        }

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

        // Now bring gamma up; beta should follow automatically.
        let _ = tasks.start_with_deps(&["gamma".to_string()]).await;
        wait_phase(&tasks, "beta", ProcessPhase::Ready).await;

        let _ = tasks.process_manager.stop_all().await;
    }

    #[tokio::test]
    async fn start_with_deps_classifies_names() {
        let (tasks, _tmp) = build_test_tasks(
            vec![long_process_task("web", vec![])],
            vec![format!("{PROCESS_TASK_PREFIX}web")],
            false,
        )
        .await;

        let parent = Arc::new(devenv_activity::start!(
            devenv_activity::Activity::operation("test").parent(None)
        ));
        let _ = tasks.run_with_parent_activity(parent).await;
        wait_phase(&tasks, "web", ProcessPhase::Ready).await;

        // Already running: untouched.
        let outcome = tasks.start_with_deps(&["web".to_string()]).await;
        assert_eq!(outcome.skipped, vec!["web".to_string()]);
        assert!(outcome.scheduled.is_empty());
        assert!(outcome.unknown.is_empty());
        assert!(outcome.failed.is_empty());

        // Not in the task graph: unknown.
        let outcome = tasks.start_with_deps(&["nosuch".to_string()]).await;
        assert_eq!(outcome.unknown, vec!["nosuch".to_string()]);
        assert!(outcome.scheduled.is_empty());
        assert!(outcome.skipped.is_empty());

        // Stopped: re-armed and scheduled.
        tasks.process_manager.stop_and_keep("web").await.unwrap();
        let outcome = tasks.start_with_deps(&["web".to_string()]).await;
        assert_eq!(outcome.scheduled, vec!["web".to_string()]);
        assert!(outcome.skipped.is_empty());
        wait_phase(&tasks, "web", ProcessPhase::Ready).await;

        let _ = tasks.process_manager.stop_all().await;
    }

    #[tokio::test]
    async fn dependency_parked_judges_live_and_transitive() {
        // Chain: beta after gamma@started, gamma after delta@started. The
        // parked judgment must be live (no stored flag) and transitive.
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
        // Register the scheduler as the daemon does, so the manager's Wait
        // settled rule can consult the live parked judgment.
        let scheduler: Arc<dyn devenv_processes::ProcessScheduler> = tasks.clone();
        tasks
            .process_manager()
            .set_scheduler(Arc::downgrade(&scheduler));

        let parent = Arc::new(devenv_activity::start!(
            devenv_activity::Activity::operation("test").parent(None)
        ));
        let _ = tasks.run_with_parent_activity(parent).await;
        wait_phase(&tasks, "delta", ProcessPhase::Ready).await;
        wait_phase(&tasks, "gamma", ProcessPhase::Ready).await;
        wait_phase(&tasks, "beta", ProcessPhase::Ready).await;

        // All Ready: nothing is parked, and Wait is settled.
        assert!(!tasks.dependency_parked("beta").await);
        assert!(tasks.process_manager.wait_settled().await);

        // Stop the whole chain, then schedule only beta: its gamma dependency
        // is Stopped, so beta is parked and Wait settles instead of hanging.
        tasks.process_manager.stop_and_keep("beta").await.unwrap();
        tasks.process_manager.stop_and_keep("gamma").await.unwrap();
        tasks.process_manager.stop_and_keep("delta").await.unwrap();

        let outcome = tasks.start_with_deps(&["beta".to_string()]).await;
        assert_eq!(outcome.scheduled, vec!["beta".to_string()]);
        assert_eq!(
            tasks.process_manager.get_phase("beta").await,
            Some(ProcessPhase::Waiting)
        );
        assert!(
            tasks.dependency_parked("beta").await,
            "beta must be parked: gamma is stopped"
        );
        assert!(
            tasks.process_manager.wait_settled().await,
            "a parked Waiting process must settle Wait"
        );

        // Schedule gamma too: it parks on stopped delta, and beta is now
        // transitively parked through gamma's Waiting entry.
        let outcome = tasks.start_with_deps(&["gamma".to_string()]).await;
        assert_eq!(outcome.scheduled, vec!["gamma".to_string()]);
        assert!(
            tasks.dependency_parked("gamma").await,
            "gamma must be parked: delta is stopped"
        );
        assert!(
            tasks.dependency_parked("beta").await,
            "beta must be transitively parked through waiting gamma"
        );
        assert!(tasks.process_manager.wait_settled().await);

        // Unpark the chain: once delta runs, gamma and beta follow and the
        // judgment flips back to progressing/launched.
        let outcome = tasks.start_with_deps(&["delta".to_string()]).await;
        assert_eq!(outcome.scheduled, vec!["delta".to_string()]);
        wait_phase(&tasks, "beta", ProcessPhase::Ready).await;
        assert!(!tasks.dependency_parked("beta").await);
        assert!(!tasks.dependency_parked("gamma").await);

        let _ = tasks.process_manager.stop_all().await;
    }

    #[tokio::test]
    async fn dependency_parked_does_not_park_on_running_oneshot() {
        // Regression: a Waiting process `p` depends on a oneshot `migrate`
        // (@succeeded); the oneshot in turn depends on a process `d` (@started).
        // `d` started, the oneshot launched and is still running, then `d` was
        // stopped. The oneshot is *progressing* (it will finish and let `p`
        // launch), so `p` must NOT be judged dependency-parked — otherwise
        // `devenv processes wait` settles early while the migration is still
        // in flight. The bug recurses into the running oneshot's now-stale
        // dependencies and wrongly concludes it is parked on stopped `d`.
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

        // `d`: registered then stopped, so its live manager phase is Stopped.
        let d_idx = tasks.task_index_by_name[&format!("{PROCESS_TASK_PREFIX}d")];
        let d_cfg = tasks.graph[d_idx]
            .read()
            .await
            .build_process_config(&tasks.env, &tasks.bash)
            .unwrap();
        tasks.process_manager.register_waiting(d_cfg, None).await;
        tasks.process_manager.cancel_waiting("d").await; // Waiting -> Stopped
        assert_eq!(
            tasks.process_manager.get_phase("d").await,
            Some(ProcessPhase::Stopped)
        );

        // The oneshot is still running (mirrors a long migration mid-flight).
        let o_idx = tasks.task_index_by_name["devenv:tasks:migrate"];
        tasks.graph[o_idx].write().await.status =
            TaskStatus::Oneshot(OneshotStatus::Running(tokio::time::Instant::now()));

        // `p` waits on a *running* oneshot, so it is progressing, not parked.
        assert!(
            !tasks.dependency_parked("p").await,
            "a process waiting on a running oneshot must not be judged parked, \
             even if a process the oneshot depended on was since stopped"
        );
    }

    #[tokio::test]
    async fn dependency_on_started_survives_explicit_stop_of_self_exited_process() {
        // Regression (code-review finding #1): process `p` starts and exits on
        // its own (reaching `Exited`). A dependent `d` declares
        // `after = ["p@started"]`. A process that exited *did* start, so
        // `p@started` is satisfied and `d` is not dependency-parked. The user
        // then explicitly stops `p` (`devenv processes stop p` / Ctrl-X).
        // Because `p` already started, `p@started` must REMAIN satisfied and
        // `d` must still not be parked.
        //
        // This currently FAILS: an explicit stop reports a plain `Stopped`
        // (terminal_phase = None), erasing the `Exited` phase that satisfied
        // `p@started`. `is_process_dep_satisfied(Stopped, Started)` is `NotYet`,
        // so `d` is wrongly judged dependency-parked and `devenv processes wait`
        // settles early while `d` never launches. (Shutdown teardown via
        // `stop_all` preserves `Exited`, so this only affects mid-session
        // explicit stops.)
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

        // Launch `p` for real and let it exit on its own.
        let p_idx = tasks.task_index_by_name[&format!("{PROCESS_TASK_PREFIX}p")];
        let p_cfg = tasks.graph[p_idx]
            .read()
            .await
            .build_process_config(&tasks.env, &tasks.bash)
            .unwrap();
        tasks
            .process_manager
            .start_command(&p_cfg, None)
            .await
            .unwrap();

        // Wait until `p` has exited on its own.
        wait_phase(&tasks, "p", ProcessPhase::Exited).await;

        // Sanity: while `p` is `Exited`, `p@started` is satisfied, so `d` is
        // not parked. (This passes today.)
        assert!(
            !tasks.dependency_parked("d").await,
            "an exited process satisfies @started, so d must not be parked"
        );

        // The user explicitly stops `p`.
        tasks.process_manager.stop_and_keep("p").await.unwrap();
        assert_eq!(
            tasks.process_manager.get_phase("p").await,
            Some(ProcessPhase::Stopped),
        );

        // `p` already started, so `d`'s `p@started` must remain satisfied and
        // `d` must NOT be judged dependency-parked. FAILS on current code.
        assert!(
            !tasks.dependency_parked("d").await,
            "a process that started then exited still satisfies @started after \
             an explicit stop; d must not be judged dependency-parked"
        );

        let _ = tasks.process_manager.stop_all().await;
    }

    #[tokio::test]
    async fn before_mode_subset_roots_skip_unrelated_process_dependency_chains() {
        let api = format!("{PROCESS_TASK_PREFIX}api");
        let db = format!("{PROCESS_TASK_PREFIX}db");
        let worker = format!("{PROCESS_TASK_PREFIX}worker");
        let blocked = format!("{PROCESS_TASK_PREFIX}blocked");

        let (tasks, _tmp) = build_test_tasks_with_run_mode(
            vec![
                process_task(&api, vec![&format!("{db}@started")]),
                process_task(&db, vec![]),
                disabled_process_task(&worker, vec![&format!("{blocked}@started")]),
                process_task(&blocked, vec![]),
            ],
            vec![api.clone()],
            RunMode::Before,
            false,
        )
        .await;

        let names = task_names(&tasks).await;
        assert!(names.contains(&api), "requested process must be scheduled");
        assert!(
            names.contains(&db),
            "requested process dependencies must be scheduled"
        );
        assert!(
            !names.contains(&worker),
            "unrelated disabled process must not become a root for a subset start"
        );
        assert!(
            !names.contains(&blocked),
            "dependencies of unrelated processes must not be scheduled"
        );
    }

    #[tokio::test]
    async fn subset_cold_start_keeps_unrelated_processes_known() {
        // A subset cold start runs only the requested closure, but the
        // scheduler must keep every configured process addressable so a later
        // `start_with_deps` (a `devenv processes start <other>` or a plain
        // `devenv up` attach) finds it instead of rejecting it as unknown.
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

        // Only the requested closure runs.
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

        // ...but every configured process stays known, so a later start finds it.
        for name in [&api, &db, &worker, &blocked] {
            assert!(
                tasks.task_index_by_name.contains_key(name),
                "{name} must remain addressable after a subset cold start"
            );
        }

        let _ = tasks.process_manager.stop_all().await;
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
