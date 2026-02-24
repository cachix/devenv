use crate::config::{Config, RunMode, parse_dependency};
use crate::error::Error;
use crate::executor::{SubprocessExecutor, TaskExecutor};
use crate::task_cache::TaskCache;
use crate::task_state::TaskState;
use crate::types::{
    DependencyKind, Output, Outputs, Skipped, TaskCompleted, TaskFailure, TaskStatus, TaskType,
    TasksStatus, VerbosityLevel,
};
use devenv_activity::{Activity, ActivityInstrument, TaskInfo, emit_task_hierarchy, next_id};
use devenv_processes::{ListenKind, NativeProcessManager};
use petgraph::algo::{has_path_connecting, toposort};
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::{EdgeRef, Reversed};
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
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
    executor: Option<Arc<dyn TaskExecutor>>,
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
            executor: None,
            refresh_task_cache: false,
        }
    }

    /// Set the database path for task caching
    pub fn with_db_path(mut self, db_path: PathBuf) -> Self {
        self.db_path = Some(db_path);
        self
    }

    /// Set a custom task executor (defaults to subprocess executor)
    pub fn with_executor(mut self, executor: Arc<dyn TaskExecutor>) -> Self {
        self.executor = Some(executor);
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
            TaskCache::with_db_path(db_path).await.map_err(|e| {
                Error::IoError(std::io::Error::other(format!(
                    "Failed to initialize task cache: {e}"
                )))
            })?
        } else {
            TaskCache::new(&self.config.cache_dir).await.map_err(|e| {
                Error::IoError(std::io::Error::other(format!(
                    "Failed to initialize task cache: {e}"
                )))
            })?
        };

        // Create process manager for long-running process tasks
        let process_manager = Arc::new(
            NativeProcessManager::new(self.config.runtime_dir.clone()).map_err(|e| {
                Error::IoError(std::io::Error::other(format!(
                    "Failed to initialize process manager: {e}"
                )))
            })?,
        );

        let mut graph = DiGraph::new();
        let mut task_indices = HashMap::new();
        let mut longest_task_name = 0;

        for task in self.config.tasks {
            let name = task.name.clone();
            longest_task_name = longest_task_name.max(name.len());
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
        let executor = self
            .executor
            .unwrap_or_else(|| Arc::new(SubprocessExecutor::new()));
        let mut tasks = Tasks {
            roots,
            root_names: self.config.roots,
            longest_task_name,
            graph,
            notify_finished: Arc::new(Notify::new()),
            notify_ui: Arc::new(Notify::new()),
            tasks_order: vec![],
            run_mode: self.config.run_mode,
            cache,
            shutdown: self.shutdown,
            process_manager,
            env: self.config.env,
            executor,
            refresh_task_cache: self.refresh_task_cache,
            ignore_process_deps: self.config.ignore_process_deps,
        };

        tasks.resolve_dependencies(task_indices).await?;
        tasks.tasks_order = tasks.schedule().await?;
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
    pub roots: Vec<NodeIndex>,
    // Stored for reporting
    pub root_names: Vec<String>,
    pub longest_task_name: usize,
    pub graph: DiGraph<Arc<RwLock<TaskState>>, DependencyKind>,
    pub tasks_order: Vec<NodeIndex>,
    pub notify_finished: Arc<Notify>,
    pub notify_ui: Arc<Notify>,
    pub run_mode: RunMode,
    pub cache: TaskCache,
    pub shutdown: Arc<tokio_shutdown::Shutdown>,
    /// Process manager for running long-lived process tasks
    pub process_manager: Arc<NativeProcessManager>,
    /// Environment variables to pass to processes
    pub env: HashMap<String, String>,
    pub executor: Arc<dyn TaskExecutor>,
    /// Force a refresh of the task cache, skipping cache reads
    pub refresh_task_cache: bool,
    /// When true, exclude non-root process-type tasks from the scheduled subgraph
    pub ignore_process_deps: bool,
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

    /// Get the current task completion status
    pub async fn get_completion_status(&self) -> TasksStatus {
        let mut status = TasksStatus::new();

        for index in &self.tasks_order {
            let task_state = self.graph[*index].read().await;
            match &task_state.status {
                TaskStatus::Pending => status.pending += 1,
                TaskStatus::Running(_) => status.running += 1,
                TaskStatus::ProcessReady => status.running += 1,
                TaskStatus::Completed(completed) => match completed {
                    TaskCompleted::Success(_, _) => status.succeeded += 1,
                    TaskCompleted::Failed(_, _) => {
                        status.failed += 1;
                        if self.is_soft_failure(index) {
                            status.soft_failed += 1;
                        }
                    }
                    TaskCompleted::Skipped(_) => status.skipped += 1,
                    TaskCompleted::DependencyFailed => {
                        status.dependency_failed += 1;
                        if self.is_soft_failure(index) {
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
    fn is_soft_failure(&self, index: &NodeIndex) -> bool {
        if self.roots.contains(index) {
            return false;
        }
        let outgoing: Vec<_> = self
            .graph
            .edges_directed(*index, petgraph::Direction::Outgoing)
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
                    {
                        let has_ready = dep_task
                            .task
                            .process
                            .as_ref()
                            .map_or(false, |p| p.ready.is_some());
                        let has_listen = dep_task.task.process.as_ref().map_or(false, |p| {
                            p.listen.iter().any(|spec| spec.kind == ListenKind::Tcp)
                        });
                        let has_ports = dep_task
                            .task
                            .process
                            .as_ref()
                            .map_or(false, |p| !p.ports.is_empty());
                        if !has_ready && !has_listen && !has_ports {
                            validation_errors.push(format!(
                                "Task '{}' depends on '{}@ready' but process has no ready config, TCP listen config, or allocated ports. \
                                 Add a ready probe, configure a TCP listen socket, or allocate ports for the process.",
                                task_state.task.name, dep_spec.name
                            ));
                        }
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
                    {
                        let has_ready = task_state
                            .task
                            .process
                            .as_ref()
                            .map_or(false, |p| p.ready.is_some());
                        let has_listen = task_state.task.process.as_ref().map_or(false, |p| {
                            p.listen.iter().any(|spec| spec.kind == ListenKind::Tcp)
                        });
                        let has_ports = task_state
                            .task
                            .process
                            .as_ref()
                            .map_or(false, |p| !p.ports.is_empty());
                        if !has_ready && !has_listen && !has_ports {
                            validation_errors.push(format!(
                                "Process '{}' has tasks depending on it via @ready but has no ready config, TCP listen config, or allocated ports. \
                                 Add a ready probe, configure a TCP listen socket, or allocate ports for the process.",
                                task_state.task.name
                            ));
                        }
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

        self.graph = subgraph;

        // Update roots to use the new node indices from the subgraph
        self.roots = self
            .roots
            .iter()
            .filter_map(|&old_index| node_map.get(&old_index).copied())
            .collect();

        // Run topological sort on the subgraph
        match toposort(&self.graph, None) {
            Ok(indexes) => Ok(indexes),
            Err(cycle) => Err(Error::CycleDetected(
                self.graph[cycle.node_id()].read().await.task.name.clone(),
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
        let orchestration_activity = Arc::new(
            Activity::operation(label)
                .detail(format!(
                    "{} {}, roots: {:?}",
                    self.tasks_order.len(),
                    item_type,
                    self.root_names
                ))
                .parent(None)
                .start(),
        );

        self.run_internal(orchestration_activity).await
    }

    /// Run with a caller-provided parent activity instead of creating a new top-level one.
    /// Used by `up()` Phase 4 to nest process execution under "Running processes".
    #[instrument(skip(self, parent_activity))]
    pub async fn run_with_parent_activity(&self, parent_activity: Arc<Activity>) -> Outputs {
        self.run_internal(parent_activity).await
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

        let outputs = Arc::new(Mutex::new(BTreeMap::new()));
        let mut running_tasks = self.shutdown.join_set();

        for index in &self.tasks_order {
            let task_state = &self.graph[*index];
            let task_activity_id = task_ids[index];

            let mut cancelled = self.shutdown.is_cancelled();
            let mut dependency_failed = false;

            // Wait for the dependencies to complete first
            if !cancelled {
                'dependency_check: loop {
                    let mut dependencies_completed = true;

                    // Check each dependency with its edge weight (Ready or Complete)
                    for edge in self
                        .graph
                        .edges_directed(*index, petgraph::Direction::Incoming)
                    {
                        let dep_index = edge.source();
                        let dep_kind = edge.weight();
                        let dep_status = &self.graph[dep_index].read().await.status;

                        let satisfied = match (dep_status, dep_kind) {
                            // @started — satisfied once running (or beyond)
                            (TaskStatus::Running(_), DependencyKind::Started) => true,
                            (TaskStatus::ProcessReady, DependencyKind::Started) => true,
                            (TaskStatus::Completed(_), DependencyKind::Started) => true,

                            // @ready — process healthy or oneshot succeeded
                            (TaskStatus::ProcessReady, DependencyKind::Ready) => true,
                            (
                                TaskStatus::Completed(TaskCompleted::Success(_, _)),
                                DependencyKind::Ready,
                            ) => true,
                            (
                                TaskStatus::Completed(TaskCompleted::Skipped(_)),
                                DependencyKind::Ready,
                            ) => true,

                            // @succeeded — exited with code 0
                            (
                                TaskStatus::Completed(TaskCompleted::Success(_, _)),
                                DependencyKind::Succeeded,
                            ) => true,
                            (
                                TaskStatus::Completed(TaskCompleted::Skipped(_)),
                                DependencyKind::Succeeded,
                            ) => true,

                            // @completed — any completion (soft)
                            (TaskStatus::Completed(_), DependencyKind::Completed) => true,

                            // Failure handling
                            (TaskStatus::Completed(completed), _) if completed.has_failed() => {
                                dependency_failed = true;
                                break 'dependency_check;
                            }

                            // Not yet satisfied
                            _ => false,
                        };

                        if !satisfied {
                            dependencies_completed = false;
                            break;
                        }
                    }

                    if dependencies_completed {
                        break;
                    }

                    tokio::select! {
                        _ = self.notify_finished.notified() => {},
                        _ = self.shutdown.wait_for_shutdown() => {
                            cancelled = true;
                            break 'dependency_check;
                        }
                    }
                }
            }

            if cancelled || dependency_failed {
                let task_completed = if cancelled {
                    TaskCompleted::Cancelled(None)
                } else {
                    TaskCompleted::DependencyFailed
                };

                // Create a minimal activity just to emit the completion event
                let skip_activity = Activity::task_with_id(task_activity_id);

                if cancelled {
                    skip_activity.cancel();
                } else {
                    skip_activity.dependency_failed();
                }

                {
                    let mut task_state = task_state.write().await;
                    task_state.status = TaskStatus::Completed(task_completed);
                }

                // Update orchestration progress
                let done = completed_tasks.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                orchestration_activity.progress(done, total_tasks, None);

                self.notify_finished.notify_one();
                self.notify_ui.notify_one();
                continue;
            }

            // Run the task

            // Check if this is a process task (long-running)
            let is_process_task = {
                let task_state = task_state.read().await;
                task_state.task.r#type == TaskType::Process
            };

            if is_process_task {
                // Process task: spawn and transition to ProcessReady immediately
                // Process activities are created as direct children of orchestration activity

                let task_state_clone = Arc::clone(task_state);
                let notify_finished_clone = Arc::clone(&self.notify_finished);
                let notify_ui_clone = Arc::clone(&self.notify_ui);
                let process_manager_clone = self.process_manager.clone();
                let parent_id = orchestration_activity.id();
                let env = &self.env;

                // Spawn the process using the process manager
                match task_state_clone
                    .write()
                    .await
                    .run_process(&process_manager_clone, Some(parent_id), env)
                    .await
                {
                    Ok(()) => {
                        // Process is now running and ready
                        notify_finished_clone.notify_one();
                        notify_ui_clone.notify_one();
                    }
                    Err(e) => {
                        // Failed to start process
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
                        notify_finished_clone.notify_one();
                        notify_ui_clone.notify_one();
                    }
                }

                // Update orchestration progress once the process is started or failed.
                let done = completed_tasks.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                orchestration_activity.progress(done, total_tasks, None);

                continue;
            }

            // Oneshot task: run once and complete
            // Reset the timer
            let now = Instant::now();

            {
                let mut task_state = task_state.write().await;
                task_state.status = TaskStatus::Running(now);
            };

            // Notify UI that task is starting
            self.notify_ui.notify_one();

            // TODO: consider Arc-ing self at this point
            let task_state_clone = Arc::clone(task_state);
            let outputs_clone = Arc::clone(&outputs);
            let notify_finished_clone = Arc::clone(&self.notify_finished);
            let notify_ui_clone = Arc::clone(&self.notify_ui);
            // TODO: remove this clone
            let cache = Arc::new(self.cache.clone());
            let shutdown_clone = Arc::clone(&self.shutdown);
            let orchestration_activity_clone = Arc::clone(&orchestration_activity);
            let completed_tasks_clone = Arc::clone(&completed_tasks);
            let executor_clone = Arc::clone(&self.executor);
            let refresh_task_cache = self.refresh_task_cache;
            let shell_env = self.env.clone();

            running_tasks.spawn(move || {
                // Clone for use inside the async block; the original is borrowed by in_activity
                let orchestration_activity_inner = Arc::clone(&orchestration_activity_clone);

                // Run the task within the orchestration activity's parent context
                // so child task activities have proper parent-child relationships and tracing spans
                async move {
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
                                executor_clone.as_ref(),
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

                    // Update orchestration progress
                    let done = completed_tasks_clone
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                        + 1;
                    orchestration_activity_inner.progress(done, total_tasks, None);

                    notify_finished_clone.notify_one();
                    notify_ui_clone.notify_one();
                }
                .in_activity(&orchestration_activity_clone)
            });
        }

        // Wait for all tasks to complete
        running_tasks.wait_all().await;

        // Check completion status and mark orchestration activity accordingly
        let status = self.get_completion_status().await;

        if status.has_failures() {
            orchestration_activity.fail();
        } else if status.cancelled > 0 {
            orchestration_activity.cancel();
        }

        self.notify_finished.notify_one();
        self.notify_ui.notify_one();

        Outputs(Arc::try_unwrap(outputs).unwrap().into_inner())
    }
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
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = tmp.path().to_path_buf();
        let runtime_dir = tmp.path().join("runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();

        let config = Config {
            tasks: task_configs,
            roots,
            run_mode: RunMode::All,
            runtime_dir,
            cache_dir,
            sudo_context: None,
            env: HashMap::new(),
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

    /// Collect task names from the scheduled tasks_order.
    async fn task_names(tasks: &Tasks) -> Vec<String> {
        let mut names = Vec::new();
        for idx in &tasks.tasks_order {
            names.push(tasks.graph[*idx].read().await.task.name.clone());
        }
        names
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
