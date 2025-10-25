use crate::config::{Config, RunMode, parse_dependency};
use crate::error::Error;
use crate::task_cache::TaskCache;
use crate::task_state::TaskState;
use crate::tracing_events;
use crate::types::{
    DependencyKind, Output, Outputs, Skipped, TaskCompleted, TaskFailure, TaskStatus, TasksStatus,
    VerbosityLevel,
};
use petgraph::algo::toposort;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
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
}

impl TasksBuilder {
    /// Create a new builder with required configuration and subsys
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
        }
    }

    /// Set the database path
    pub fn with_db_path(mut self, db_path: PathBuf) -> Self {
        self.db_path = Some(db_path);
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
            TaskCache::new().await.map_err(|e| {
                Error::IoError(std::io::Error::other(format!(
                    "Failed to initialize task cache: {e}"
                )))
            })?
        };

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
                    TaskCompleted::Failed(_, _) => status.failed += 1,
                    TaskCompleted::Skipped(_) => status.skipped += 1,
                    TaskCompleted::DependencyFailed => status.dependency_failed += 1,
                    TaskCompleted::Cancelled(_) => status.cancelled += 1,
                },
            }
        }

        status
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

        for index in self.graph.node_indices() {
            let task_state = &self.graph[index].read().await;

            for dep_name in &task_state.task.after {
                // Parse dependency with optional suffix
                let dep_spec = parse_dependency(dep_name)?;

                if let Some(dep_idx) = task_indices.get(&dep_spec.name) {
                    edges_to_add.push((*dep_idx, index, dep_spec.kind));
                } else {
                    unresolved.insert((task_state.task.name.clone(), dep_name.clone()));
                }
            }

            for before_name in &task_state.task.before {
                // Parse dependency with optional suffix
                let dep_spec = parse_dependency(before_name)?;

                if let Some(before_idx) = task_indices.get(&dep_spec.name) {
                    edges_to_add.push((index, *before_idx, dep_spec.kind));
                } else {
                    unresolved.insert((task_state.task.name.clone(), before_name.clone()));
                }
            }
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
                // Include the complete connected subgraph (all dependencies in both directions)
                while let Some(node) = to_visit.pop() {
                    if visited.insert(node) {
                        // Add all connected neighbors in both directions
                        for neighbor in self.graph.neighbors_undirected(node) {
                            to_visit.push(neighbor);
                        }
                    }
                }
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
    pub async fn run(&self) -> Outputs {
        // Create an orchestration-level span to track the overall task execution
        let orchestration_span = tracing::info_span!(
            "devenv_tasks_orchestration",
            total_tasks = self.tasks_order.len(),
            root_tasks = ?self.root_names,
        );
        let _orchestration_guard = orchestration_span.enter();

        let outputs = Arc::new(Mutex::new(BTreeMap::new()));
        let mut running_tasks = self.shutdown.join_set();

        for index in &self.tasks_order {
            let task_state = &self.graph[*index];

            let mut cancelled = self.shutdown.is_cancelled();
            let mut dependency_failed = false;

            // Wait for the dependencies to complete first
            if !cancelled {
                'dependency_check: loop {
                    let mut dependencies_completed = true;

                    for dep_index in self
                        .graph
                        .neighbors_directed(*index, petgraph::Direction::Incoming)
                    {
                        match &self.graph[dep_index].read().await.status {
                            TaskStatus::Completed(completed) => {
                                if completed.has_failed() {
                                    dependency_failed = true;
                                    break 'dependency_check;
                                }
                            }
                            TaskStatus::ProcessReady => {
                                // Process is ready and healthy, dependency is satisfied
                            }
                            TaskStatus::Pending => {
                                dependencies_completed = false;
                                break;
                            }
                            TaskStatus::Running(_) => {
                                dependencies_completed = false;
                                break;
                            }
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

            let task_name = {
                let task_state = task_state.read().await;
                // TODO: remove clone
                task_state.task.name.clone()
            };

            if cancelled || dependency_failed {
                let task_completed = if cancelled {
                    TaskCompleted::Cancelled(None)
                } else {
                    TaskCompleted::DependencyFailed
                };

                let tracing_status = task_completed.to_tracing_status();

                tracing_events::emit_task_completed(
                    &task_name,
                    "completed",
                    tracing_status,
                    None,
                    Some(tracing_status),
                );

                {
                    let mut task_state = task_state.write().await;
                    task_state.status = TaskStatus::Completed(task_completed);
                }

                self.notify_finished.notify_one();
                self.notify_ui.notify_one();
                continue;
            }

            // Run the task

            // Reset the timer
            let now = Instant::now();

            {
                let mut task_state = task_state.write().await;
                task_state.status = TaskStatus::Running(now);
            };

            // TODO: do we need two separate events here?
            tracing_events::emit_task_start(&task_name);
            tracing_events::emit_task_status_change(&task_name, "running", None);

            self.notify_ui.notify_one();

            // TODO: consider Arc-ing self at this point
            let task_state_clone = Arc::clone(task_state);
            let outputs_clone = Arc::clone(&outputs);
            let notify_finished_clone = Arc::clone(&self.notify_finished);
            let notify_ui_clone = Arc::clone(&self.notify_ui);
            // TODO: remove this clone
            let cache = Arc::new(self.cache.clone());
            let shutdown_clone = Arc::clone(&self.shutdown);

            running_tasks.spawn(move || async move {
                let completed = {
                    let outputs = outputs_clone.lock().await.clone();
                    match task_state_clone
                        .read()
                        .await
                        .run(now, &outputs, &cache, shutdown_clone.cancellation_token())
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
                                    error: format!("Task failed: {e}"),
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

                    let task_name = &task_state.task.name;

                    // Emit comprehensive tracing event for completion
                    match &completed {
                        TaskCompleted::Success(duration, _) => {
                            tracing_events::emit_task_completed(
                                task_name,
                                "completed",
                                "success",
                                Some(duration.as_secs_f64()),
                                None,
                            );
                        }
                        TaskCompleted::Failed(duration, _) => {
                            tracing_events::emit_task_completed(
                                task_name,
                                "completed",
                                "failed",
                                Some(duration.as_secs_f64()),
                                None,
                            );
                        }
                        TaskCompleted::Skipped(skipped) => match skipped {
                            Skipped::Cached(_) => {
                                tracing_events::emit_task_completed(
                                    task_name,
                                    "completed",
                                    "cached",
                                    None,
                                    Some("cached"),
                                );
                            }
                            Skipped::NoCommand => {
                                tracing_events::emit_task_completed(
                                    task_name,
                                    "completed",
                                    "skipped",
                                    None,
                                    Some("no command"),
                                );
                            }
                        },
                        TaskCompleted::DependencyFailed => {
                            // Already emitted above
                        }
                        TaskCompleted::Cancelled(duration) => {
                            tracing_events::emit_task_completed(
                                task_name,
                                "completed",
                                "cancelled",
                                duration.map(|d| d.as_secs_f64()),
                                Some("cancelled"),
                            );
                        }
                    }

                    task_state.status = TaskStatus::Completed(completed);
                }

                notify_finished_clone.notify_one();
                notify_ui_clone.notify_one();
            });
        }

        // Wait for all tasks to complete
        running_tasks.wait_all().await;

        // Emit final progress event showing all tasks completed
        tracing::info!(
            progress.type = "generic",
            progress.current = self.tasks_order.len(),
            progress.total = self.tasks_order.len(),
            "All tasks completed"
        );

        self.notify_finished.notify_one();
        self.notify_ui.notify_one();

        Outputs(Arc::try_unwrap(outputs).unwrap().into_inner())
    }
}
