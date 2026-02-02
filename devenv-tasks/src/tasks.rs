use crate::config::{Config, RunMode, parse_dependency};
use crate::error::Error;
use crate::task_cache::TaskCache;
use crate::task_state::TaskState;
use crate::types::{
    DependencyKind, Output, Outputs, Skipped, TaskCompleted, TaskFailure, TaskStatus, TasksStatus,
    VerbosityLevel,
};
use devenv_activity::{Activity, ActivityInstrument, TaskInfo, emit_task_hierarchy, next_id};
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
        // Create an orchestration-level Operation activity to track overall progress
        // Using Operation (not Task) so it doesn't count in the task summary
        let orchestration_activity = Arc::new(
            Activity::operation("Running tasks")
                .detail(format!(
                    "{} tasks, roots: {:?}",
                    self.tasks_order.len(),
                    self.root_names
                ))
                .parent(None)
                .start(),
        );

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

                    for edge in self
                        .graph
                        .edges_directed(*index, petgraph::Direction::Incoming)
                    {
                        let dep_index = edge.source();
                        let dep_kind = edge.weight();

                        match &self.graph[dep_index].read().await.status {
                            TaskStatus::Completed(completed) => {
                                // Only propagate failure for Ready/Succeeded dependencies
                                // Complete dependencies just wait for the task to finish (soft dependency)
                                if *dep_kind != DependencyKind::Complete && completed.has_failed() {
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

        if status.failed > 0 || status.dependency_failed > 0 {
            orchestration_activity.fail();
        } else if status.cancelled > 0 {
            orchestration_activity.cancel();
        }

        self.notify_finished.notify_one();
        self.notify_ui.notify_one();

        Outputs(Arc::try_unwrap(outputs).unwrap().into_inner())
    }
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
