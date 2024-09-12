use crossterm::{
    cursor, execute,
    style::{self, Stylize},
    terminal::{Clear, ClearType},
};
use miette::Diagnostic;
use petgraph::algo::toposort;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
#[cfg(test)]
use pretty_assertions::assert_matches;
use serde::{Deserialize, Serialize};
#[cfg(test)]
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::fmt::Display;
#[cfg(test)]
use std::fs;
use std::io;
#[cfg(test)]
use std::io::Write;
#[cfg(test)]
use std::os::unix::fs::PermissionsExt;
use std::process::Stdio;
use std::sync::Arc;
use test_log::test;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::RwLock;
use tokio::task::JoinSet;
use tokio::time::{Duration, Instant};
use tracing::{error, info, instrument};

#[derive(Error, Diagnostic, Debug)]
pub enum Error {
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    TaskNotFound(String),
    MissingCommand(String),
    TasksNotFound(Vec<(String, String)>),
    InvalidTaskName(String),
    // TODO: be more precies where the cycle happens
    CycleDetected(String),
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::IoError(e) => write!(f, "IO Error: {}", e),
            Error::TasksNotFound(tasks) => write!(
                f,
                "Task dependencies not found: {}",
                tasks
                    .iter()
                    .map(|(task, dep)| format!("{} is depending on non-existent {}", task, dep))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Error::TaskNotFound(task) => write!(f, "Task does not exist: {}", task),
            Error::CycleDetected(task) => write!(f, "Cycle detected at task: {}", task),
            Error::MissingCommand(task) => write!(
                f,
                "Task {} defined a status, but is missing a command",
                task
            ),
            Error::InvalidTaskName(task) => write!(
                f,
                "Invalid task name: {}, expected [a-zA-Z-_]+:[a-zA-Z-_]+",
                task
            ),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TaskConfig {
    name: String,
    #[serde(default)]
    depends: Vec<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Deserialize, Serialize)]
pub struct Config {
    pub tasks: Vec<TaskConfig>,
    pub roots: Vec<String>,
}

impl TryFrom<serde_json::Value> for Config {
    type Error = serde_json::Error;

    fn try_from(json: serde_json::Value) -> Result<Self, Self::Error> {
        serde_json::from_value(json)
    }
}

type Output = Vec<(std::time::Instant, String)>;

#[derive(Debug, Clone)]
struct TaskFailure {
    stdout: Output,
    stderr: Output,
    error: String,
}

#[derive(Debug, Clone)]
enum Skipped {
    Cached,
    NotImplemented,
}

#[derive(Debug, Clone)]
enum TaskCompleted {
    Success(Duration),
    Skipped(Skipped),
    Failed(Duration, TaskFailure),
    DependencyFailed,
}

impl TaskCompleted {
    fn has_failed(&self) -> bool {
        matches!(
            self,
            TaskCompleted::Failed(_, _) | TaskCompleted::DependencyFailed
        )
    }
}

#[derive(Debug, Clone)]
enum TaskStatus {
    Pending,
    Running(Instant),
    Completed(TaskCompleted),
}

#[derive(Debug)]
struct TaskState {
    task: TaskConfig,
    status: TaskStatus,
}

impl TaskState {
    fn new(task: TaskConfig) -> Self {
        Self {
            task,
            status: TaskStatus::Pending,
        }
    }

    #[instrument(ret)]
    async fn run(&self, now: Instant) -> TaskCompleted {
        if let Some(cmd) = &self.task.status {
            let result = Command::new(cmd)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .status()
                .await;
            match result {
                Ok(status) => {
                    if status.success() {
                        return TaskCompleted::Skipped(Skipped::Cached);
                    }
                }
                Err(e) => {
                    // TODO: stdout, stderr
                    return TaskCompleted::Failed(
                        now.elapsed(),
                        TaskFailure {
                            stdout: Vec::new(),
                            stderr: Vec::new(),
                            error: e.to_string(),
                        },
                    );
                }
            }
        }
        if let Some(cmd) = &self.task.command {
            let result = Command::new(cmd)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn();

            let mut child = match result {
                Ok(c) => c,
                Err(e) => {
                    return TaskCompleted::Failed(
                        now.elapsed(),
                        TaskFailure {
                            stdout: Vec::new(),
                            stderr: Vec::new(),
                            error: e.to_string(),
                        },
                    );
                }
            };

            let stdout = match child.stdout.take() {
                Some(stdout) => stdout,
                None => {
                    return TaskCompleted::Failed(
                        now.elapsed(),
                        TaskFailure {
                            stdout: Vec::new(),
                            stderr: Vec::new(),
                            error: "Failed to capture stdout".to_string(),
                        },
                    )
                }
            };
            let stderr = match child.stderr.take() {
                Some(stderr) => stderr,
                None => {
                    return TaskCompleted::Failed(
                        now.elapsed(),
                        TaskFailure {
                            stdout: Vec::new(),
                            stderr: Vec::new(),
                            error: "Failed to capture stderr".to_string(),
                        },
                    )
                }
            };

            let mut stderr_reader = BufReader::new(stderr).lines();
            let mut stdout_reader = BufReader::new(stdout).lines();

            let mut stdout_lines = Vec::new();
            let mut stderr_lines = Vec::new();

            loop {
                tokio::select! {
                    result = stdout_reader.next_line() => {
                        match result {
                            Ok(Some(line)) => {
                                info!(stdout = %line);
                                stdout_lines.push((std::time::Instant::now(), line));
                            },
                            Ok(None) => {},
                            Err(e) => {
                                error!("Error reading stdout: {}", e);
                                stderr_lines.push((std::time::Instant::now(), e.to_string()));
                            },
                        }
                    }
                    result = stderr_reader.next_line() => {
                        match result {
                            Ok(Some(line)) => {
                                stderr_lines.push((std::time::Instant::now(), line));
                            },
                            Ok(None) => {},
                            Err(e) => {
                                stderr_lines.push((std::time::Instant::now(), e.to_string()));
                            },
                        }
                    }
                    result = child.wait() => {
                        match result {
                            Ok(status) => {
                                if status.success() {
                                    return TaskCompleted::Success(now.elapsed());
                                } else {
                                    return TaskCompleted::Failed(
                                        now.elapsed(),
                                        TaskFailure {
                                            stdout: stdout_lines,
                                            stderr: stderr_lines,
                                            error: format!("Task exited with status: {}", status),
                                        },
                                    );
                                }
                            },
                            Err(e) => {
                                error!("Error waiting for command: {}", e);
                                return TaskCompleted::Failed(
                                    now.elapsed(),
                                    TaskFailure {
                                        stdout: stdout_lines,
                                        stderr: stderr_lines,
                                        error: format!("Error waiting for command: {}", e),
                                    },
                                );
                            }
                        }
                    }
                }
            }
        } else {
            return TaskCompleted::Skipped(Skipped::NotImplemented);
        }
    }
}

#[derive(Debug)]
struct Tasks {
    roots: Vec<NodeIndex>,
    // Stored for reporting
    root_names: Vec<String>,
    longest_task_name: usize,
    graph: DiGraph<Arc<RwLock<TaskState>>, ()>,
    tasks_order: Vec<NodeIndex>,
}

impl Tasks {
    async fn new(config: Config) -> Result<Self, Error> {
        let mut graph = DiGraph::new();
        let mut task_indices = HashMap::new();
        let mut longest_task_name = 0;
        for task in config.tasks {
            let name = task.name.clone();
            longest_task_name = longest_task_name.max(name.len());
            if !task.name.contains(':')
                || task.name.split(':').count() < 2
                || task.name.starts_with(':')
                || task.name.ends_with(':')
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
            let index = graph.add_node(Arc::new(RwLock::new(TaskState::new(task))));
            task_indices.insert(name, index);
        }
        let mut roots = Vec::new();
        for name in config.roots.clone() {
            if let Some(index) = task_indices.get(&name) {
                roots.push(*index);
            } else {
                return Err(Error::TaskNotFound(name));
            }
        }
        let mut tasks = Self {
            roots,
            root_names: config.roots,
            longest_task_name,
            graph,
            tasks_order: vec![],
        };
        tasks.resolve_dependencies(task_indices).await?;
        tasks.tasks_order = tasks.schedule().await?;
        Ok(tasks)
    }

    async fn resolve_dependencies(
        &mut self,
        task_indices: HashMap<String, NodeIndex>,
    ) -> Result<(), Error> {
        let mut unresolved = HashSet::new();
        let mut edges_to_add = Vec::new();

        for index in self.graph.node_indices() {
            let task_state = &self.graph[index].read().await;

            for dep_name in &task_state.task.depends {
                if let Some(dep_idx) = task_indices.get(dep_name) {
                    edges_to_add.push((*dep_idx, index));
                } else {
                    unresolved.insert((task_state.task.name.clone(), dep_name.clone()));
                }
            }
        }

        for (dep_idx, idx) in edges_to_add {
            self.graph.add_edge(dep_idx, idx, ());
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

        // Depth-first search including dependencies
        while let Some(node) = to_visit.pop() {
            if visited.insert(node) {
                let new_node = subgraph.add_node(self.graph[node].clone());
                node_map.insert(node, new_node);

                // Add dependencies to visit
                for neighbor in self
                    .graph
                    .neighbors_directed(node, petgraph::Direction::Incoming)
                {
                    to_visit.push(neighbor);
                }
            }
        }

        // Add edges to subgraph
        for (&old_node, &new_node) in &node_map {
            for edge in self.graph.edges(old_node) {
                let target = edge.target();
                if let Some(&new_target) = node_map.get(&target) {
                    subgraph.add_edge(new_node, new_target, ());
                }
            }
        }

        self.graph = subgraph;

        // Run topological sort on the subgraph
        match toposort(&self.graph, None) {
            Ok(indexes) => Ok(indexes),
            Err(cycle) => Err(Error::CycleDetected(
                self.graph[cycle.node_id()].read().await.task.name.clone(),
            )),
        }
    }

    #[instrument(skip(self))]
    async fn run(&self) {
        let mut running_tasks = JoinSet::new();

        for index in &self.tasks_order {
            let task_state = &self.graph[*index];

            let mut dependency_failed = false;

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

                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }

            if dependency_failed {
                let mut task_state = task_state.write().await;
                task_state.status = TaskStatus::Completed(TaskCompleted::DependencyFailed);
            } else {
                let now = Instant::now();

                // hold write lock only to update the status
                {
                    let mut task_state = task_state.write().await;
                    task_state.status = TaskStatus::Running(now);
                }

                let task_state_clone = Arc::clone(task_state);
                running_tasks.spawn(async move {
                    let completed = task_state_clone.read().await.run(now).await;
                    {
                        let mut task_state = task_state_clone.write().await;
                        task_state.status = TaskStatus::Completed(completed);
                    }
                });
            }
        }

        while let Some(res) = running_tasks.join_next().await {
            match res {
                Ok(_) => (),
                Err(e) => eprintln!("Task crashed: {}", e),
            }
        }
    }
}

pub struct TasksStatus {
    lines: Vec<String>,
    pub pending: usize,
    pub running: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub skipped: usize,
    pub dependency_failed: usize,
}

impl TasksStatus {
    fn new() -> Self {
        Self {
            lines: vec![],
            pending: 0,
            running: 0,
            succeeded: 0,
            failed: 0,
            skipped: 0,
            dependency_failed: 0,
        }
    }
}

pub struct TasksUi {
    tasks: Arc<Tasks>,
}

impl TasksUi {
    pub async fn new(config: Config) -> Result<Self, Error> {
        let tasks = Tasks::new(config).await?;
        Ok(Self {
            tasks: Arc::new(tasks),
        })
    }

    async fn get_tasks_status(&self) -> TasksStatus {
        let mut tasks_status = TasksStatus::new();

        for index in &self.tasks.tasks_order {
            let (task_status, task_name) = {
                let task_state = self.tasks.graph[*index].read().await;
                (task_state.status.clone(), task_state.task.name.clone())
            };
            let (status_text, duration) = match task_status {
                TaskStatus::Pending => {
                    tasks_status.pending += 1;
                    continue;
                }
                TaskStatus::Running(started) => {
                    tasks_status.running += 1;
                    (
                        format!("{:17}", "Running").blue().bold(),
                        Some(started.elapsed()),
                    )
                }
                TaskStatus::Completed(TaskCompleted::Skipped(skipped)) => {
                    tasks_status.skipped += 1;
                    let status = match skipped {
                        Skipped::Cached => "Cached",
                        Skipped::NotImplemented => "Not implemented",
                    };
                    (format!("{:17}", status).blue().bold(), None)
                }
                TaskStatus::Completed(TaskCompleted::Success(duration)) => {
                    tasks_status.succeeded += 1;
                    (format!("{:17}", "Succeeded").green().bold(), Some(duration))
                }
                TaskStatus::Completed(TaskCompleted::Failed(duration, _)) => {
                    tasks_status.failed += 1;
                    (format!("{:17}", "Failed").red().bold(), Some(duration))
                }
                TaskStatus::Completed(TaskCompleted::DependencyFailed) => {
                    tasks_status.dependency_failed += 1;
                    (format!("{:17}", "Dependency failed").magenta().bold(), None)
                }
            };

            let duration = match duration {
                Some(d) => d.as_millis().to_string() + "ms",
                None => "".to_string(),
            };
            tasks_status.lines.push(format!(
                "{} {} {}",
                status_text,
                format!("{:width$}", task_name, width = self.tasks.longest_task_name).bold(),
                duration,
            ));
        }

        tasks_status
    }

    pub async fn run(&mut self) -> Result<TasksStatus, Error> {
        let mut stdout = io::stdout();
        let names = self.tasks.root_names.join(", ").bold();

        let started = std::time::Instant::now();

        // start processing tasks
        let tasks_clone = Arc::clone(&self.tasks);
        let handle = tokio::spawn(async move { tasks_clone.run().await });

        // start TUI

        let mut last_list_height: u16 = 0;

        loop {
            let mut finished = false;
            if handle.is_finished() {
                finished = true;
            }

            let tasks_status = self.get_tasks_status().await;

            execute!(
                stdout,
                // Clear the screen from the cursor down
                cursor::MoveUp(last_list_height),
                Clear(ClearType::FromCursorDown),
                style::PrintStyledContent(
                    format!(
                        "{}\n{} {}: {}\n",
                        tasks_status.lines.join("\n"),
                        if finished {
                            format!("Finished in {:.2?}", started.elapsed())
                        } else {
                            format!("Running for {:.2?}", started.elapsed())
                        },
                        names.clone(),
                        [
                            if tasks_status.pending > 0 {
                                format!("{} {}", tasks_status.pending, "Pending".blue().bold())
                            } else {
                                String::new()
                            },
                            if tasks_status.running > 0 {
                                format!("{} {}", tasks_status.running, "Running".blue().bold())
                            } else {
                                String::new()
                            },
                            if tasks_status.skipped > 0 {
                                format!("{} {}", tasks_status.skipped, "Skipped".blue().bold())
                            } else {
                                String::new()
                            },
                            if tasks_status.succeeded > 0 {
                                format!("{} {}", tasks_status.succeeded, "Succeeded".green().bold())
                            } else {
                                String::new()
                            },
                            if tasks_status.failed > 0 {
                                format!("{} {}", tasks_status.failed, "Failed".red().bold())
                            } else {
                                String::new()
                            },
                            if tasks_status.dependency_failed > 0 {
                                format!(
                                    "{} {}",
                                    tasks_status.dependency_failed,
                                    "Dependency Failed".red().bold()
                                )
                            } else {
                                String::new()
                            },
                        ]
                        .into_iter()
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join(", ")
                    )
                    .stylize()
                ),
            )?;

            if finished {
                break;
            }

            last_list_height = tasks_status.lines.len() as u16 + 1;

            // Sleep briefly to avoid excessive redraws
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        let errors = {
            let mut errors = String::new();
            for index in &self.tasks.tasks_order {
                let task_state = self.tasks.graph[*index].read().await;
                if let TaskStatus::Completed(TaskCompleted::Failed(_, failure)) = &task_state.status
                {
                    errors.push_str(&format!(
                        "\n--- {} failed with error: {}\n",
                        task_state.task.name, failure.error
                    ));
                    errors.push_str(&format!("--- {} stdout:\n", task_state.task.name));
                    for (time, line) in &failure.stdout {
                        errors.push_str(&format!(
                            "{:07.2}: {}\n",
                            time.elapsed().as_secs_f32(),
                            line
                        ));
                    }
                    errors.push_str(&format!("--- {} stderr:\n", task_state.task.name));
                    for (time, line) in &failure.stderr {
                        errors.push_str(&format!(
                            "{:07.2}: {}\n",
                            time.elapsed().as_secs_f32(),
                            line
                        ));
                    }
                    errors.push_str("---\n")
                }
            }
            errors.stylize()
        };
        execute!(stdout, style::PrintStyledContent(errors))?;

        let tasks_status = self.get_tasks_status().await;
        Ok(tasks_status)
    }
}

#[test(tokio::test)]
async fn test_task_name() -> Result<(), Error> {
    let invalid_names = vec![
        "invalid:name!",
        "invalid name",
        "invalid@name",
        ":invalid",
        "invalid:",
        "invalid",
    ];
    for task in invalid_names {
        assert_matches!(
            Config::try_from(json!({
                "roots": [],
                    "tasks": [{
                        "name": task.to_string()
                    }]
            }))
            .map(Tasks::new)
            .unwrap()
            .await,
            Err(Error::InvalidTaskName(_))
        );
    }
    let valid_names = vec![
        "devenv:enterShell",
        "devenv:enter-shell",
        "devenv:enter_shell",
        "devenv:python:virtualenv",
    ];
    for task in valid_names {
        assert_matches!(
            Config::try_from(serde_json::json!({
                "roots": [],
                "tasks": [{
                    "name": task.to_string()
                }]
            }))
            .map(Tasks::new)
            .unwrap()
            .await,
            Ok(_)
        );
    }
    Ok(())
}

#[test(tokio::test)]
async fn test_basic_tasks() -> Result<(), Error> {
    let script1 = create_script(
        "#!/bin/sh\necho 'Task 1 is running' && sleep 0.5 && echo 'Task 1 completed'",
    )?;
    let script2 = create_script(
        "#!/bin/sh\necho 'Task 2 is running' && sleep 0.5 && echo 'Task 2 completed'",
    )?;
    let script3 = create_script(
        "#!/bin/sh\necho 'Task 3 is running' && sleep 0.5 && echo 'Task 3 completed'",
    )?;
    let script4 = create_script("#!/bin/sh\necho 'Task 4 is running' && echo 'Task 4 completed'")?;

    let tasks = Tasks::new(
        Config::try_from(json!({
            "roots": ["myapp:task_1", "myapp:task_4"],
            "tasks": [
                {
                    "name": "myapp:task_1",
                    "command": script1.to_str().unwrap()
                },
                {
                    "name": "myapp:task_2",
                    "command": script2.to_str().unwrap()
                },
                {
                    "name": "myapp:task_3",
                    "depends": ["myapp:task_1"],
                    "command": script3.to_str().unwrap()
                },
                {
                    "name": "myapp:task_4",
                    "depends": ["myapp:task_3"],
                    "command": script4.to_str().unwrap()
                }
            ]
        }))
        .unwrap(),
    )
    .await?;
    tasks.run().await;

    let task_statuses = inspect_tasks(&tasks).await;
    let task_statuses = task_statuses.as_slice();
    assert_matches!(
        task_statuses,
        [
            (name1, TaskStatus::Completed(TaskCompleted::Success(_))),
            (name2, TaskStatus::Completed(TaskCompleted::Success(_))),
            (name3, TaskStatus::Completed(TaskCompleted::Success(_)))
        ] if name1 == "myapp:task_1" && name2 == "myapp:task_3" && name3 == "myapp:task_4"
    );
    Ok(())
}

#[test(tokio::test)]
async fn test_tasks_cycle() -> Result<(), Error> {
    let result = Tasks::new(
        Config::try_from(json!({
            "roots": ["myapp:task_1"],
            "tasks": [
                {
                    "name": "myapp:task_1",
                    "depends": ["myapp:task_2"],
                    "command": "echo 'Task 1 is running' && echo 'Task 1 completed'"
                },
                {
                    "name": "myapp:task_2",
                    "depends": ["myapp:task_1"],
                    "command": "echo 'Task 2 is running' && echo 'Task 2 completed'"
                }
            ]
        }))
        .unwrap(),
    )
    .await;
    if let Err(Error::CycleDetected(task)) = result {
        assert_eq!(task, "myapp:task_2".to_string());
    } else {
        panic!("Expected Error::CycleDetected, got {:?}", result);
    }
    Ok(())
}

#[test(tokio::test)]
async fn test_status() -> Result<(), Error> {
    let command_script1 =
        create_script("#!/bin/sh\necho 'Task 1 is running' && echo 'Task 1 completed'")?;
    let status_script1 = create_script("#!/bin/sh\nexit 0")?;
    let command_script2 =
        create_script("#!/bin/sh\necho 'Task 2 is running' && echo 'Task 2 completed'")?;
    let status_script2 = create_script("#!/bin/sh\nexit 1")?;

    let command1 = command_script1.to_str().unwrap();
    let status1 = status_script1.to_str().unwrap();
    let command2 = command_script2.to_str().unwrap();
    let status2 = status_script2.to_str().unwrap();

    let create_tasks = |root: &'static str| async move {
        Tasks::new(
            Config::try_from(json!({
                "roots": [root],
                "tasks": [
                    {
                        "name": "myapp:task_1",
                        "command": command1,
                        "status": status1
                    },
                    {
                        "name": "myapp:task_2",
                        "command": command2,
                        "status": status2
                    }
                ]
            }))
            .unwrap(),
        )
        .await
    };

    let tasks = create_tasks("myapp:task_1").await.unwrap();
    tasks.run().await;
    assert_eq!(tasks.tasks_order.len(), 1);
    assert_matches!(
        tasks.graph[tasks.tasks_order[0]].read().await.status,
        TaskStatus::Completed(TaskCompleted::Skipped(Skipped::Cached))
    );

    let tasks = create_tasks("myapp:task_2").await.unwrap();
    tasks.run().await;
    assert_eq!(tasks.tasks_order.len(), 1);
    assert_matches!(
        tasks.graph[tasks.tasks_order[0]].read().await.status,
        TaskStatus::Completed(TaskCompleted::Success(_))
    );

    Ok(())
}

#[test(tokio::test)]
async fn test_nonexistent_script() -> Result<(), Error> {
    let tasks = Tasks::new(
        Config::try_from(json!({
            "roots": ["myapp:task_1"],
            "tasks": [
                {
                    "name": "myapp:task_1",
                    "command": "/path/to/nonexistent/script.sh"
                }
            ]
        }))
        .unwrap(),
    )
    .await?;

    tasks.run().await;

    let task_statuses = inspect_tasks(&tasks).await;
    let task_statuses = task_statuses.as_slice();
    assert_matches!(
        &task_statuses,
        [(
            task_1,
            TaskStatus::Completed(TaskCompleted::Failed(
                _,
                TaskFailure {
                    stdout: _,
                    stderr: _,
                    error
                }
            ))
        )] if error == "No such file or directory (os error 2)" && task_1 == "myapp:task_1"
    );

    Ok(())
}

#[test(tokio::test)]
async fn test_status_without_command() -> Result<(), Error> {
    let status_script = create_script("#!/bin/sh\nexit 0")?;

    let result = Tasks::new(
        Config::try_from(json!({
            "roots": ["myapp:task_1"],
            "tasks": [
                {
                    "name": "myapp:task_1",
                    "status": status_script.to_str().unwrap()
                }
            ]
        }))
        .unwrap(),
    )
    .await;

    assert!(matches!(result, Err(Error::MissingCommand(_))));
    Ok(())
}

#[test(tokio::test)]
async fn test_dependency_failure() -> Result<(), Error> {
    let failing_script = create_script("#!/bin/sh\necho 'Failing task' && exit 1")?;
    let dependent_script = create_script("#!/bin/sh\necho 'Dependent task' && exit 0")?;

    let tasks = Tasks::new(
        Config::try_from(json!({
            "roots": ["myapp:task_2"],
            "tasks": [
                {
                    "name": "myapp:task_1",
                    "command": failing_script.to_str().unwrap()
                },
                {
                    "name": "myapp:task_2",
                    "depends": ["myapp:task_1"],
                    "command": dependent_script.to_str().unwrap()
                }
            ]
        }))
        .unwrap(),
    )
    .await?;

    tasks.run().await;

    let task_statuses = inspect_tasks(&tasks).await;
    let task_statuses_slice = &task_statuses.as_slice();
    assert_matches!(
        *task_statuses_slice,
        [
            (task_1, TaskStatus::Completed(TaskCompleted::Failed(_, _))),
            (
                task_2,
                TaskStatus::Completed(TaskCompleted::DependencyFailed)
            )
        ] if task_1 == "myapp:task_1" && task_2 == "myapp:task_2"
    );

    Ok(())
}

#[cfg(test)]
async fn inspect_tasks(tasks: &Tasks) -> Vec<(String, TaskStatus)> {
    let mut result = Vec::new();
    for index in &tasks.tasks_order {
        let task_state = tasks.graph[*index].read().await;
        result.push((task_state.task.name.clone(), task_state.status.clone()));
    }
    result
}

#[cfg(test)]
fn create_script(script: &str) -> std::io::Result<tempfile::TempPath> {
    let mut temp_file = tempfile::Builder::new()
        .prefix(&format!("script"))
        .suffix(".sh")
        .tempfile()?;
    temp_file.write_all(script.as_bytes())?;
    temp_file
        .as_file_mut()
        .set_permissions(fs::Permissions::from_mode(0o755))?;
    Ok(temp_file.into_temp_path())
}
