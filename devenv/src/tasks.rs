use assert_matches::assert_matches;
use crossterm::{
    cursor, execute,
    style::{self, Stylize},
    terminal::{Clear, ClearType},
};
use miette::Diagnostic;
use petgraph::algo::toposort;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::{Dfs, EdgeRef};
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{self, Write};
use std::process::Stdio;
use std::sync::Arc;
use std::{
    collections::{HashMap, HashSet},
    fs,
};
use std::{fmt::Display, os::unix::fs::PermissionsExt};
use test_log::test;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinSet;
use tokio::time::{Duration, Instant};
use tracing::{error, info, instrument};

#[derive(Error, Diagnostic, Debug)]
pub enum Error {
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    TaskNotFound(String),
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
            Error::InvalidTaskName(task) => write!(
                f,
                "Invalid task name: {}, expected [a-zA-Z-_]:[a-zA-Z-_]",
                task
            ),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct TaskConfig {
    name: String,
    #[serde(default)]
    depends: Vec<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Deserialize)]
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

#[derive(Debug)]
enum TaskCompleted {
    Success(Duration),
    Skipped,
    Failed(Duration),
    DependencyFailed,
}

impl TaskCompleted {
    fn has_failed(&self) -> bool {
        matches!(
            self,
            TaskCompleted::Failed(_) | TaskCompleted::DependencyFailed
        )
    }
}

#[derive(Debug)]
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

    #[instrument]
    async fn run(&mut self) -> TaskCompleted {
        let now = Instant::now();
        self.status = TaskStatus::Running(now);
        if let Some(status) = &self.task.status {
            let mut child = Command::new(status)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("Failed to execute status");

            match child.wait().await {
                Err(_) => {}
                Ok(status) => {
                    if status.success() {
                        return TaskCompleted::Skipped;
                    }
                }
            }
        }
        if let Some(cmd) = &self.task.command {
            let mut child = Command::new(cmd)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("Failed to execute command");

            let stdout = child.stdout.take().expect("Failed to open stdout");
            let stderr = child.stderr.take().expect("Failed to open stderr");

            let mut stderr_reader = BufReader::new(stderr).lines();
            let mut stdout_reader = BufReader::new(stdout).lines();

            loop {
                tokio::select! {
                    result = stdout_reader.next_line() => {
                        match result {
                            Ok(Some(line)) => info!(stdout = %line),
                            Ok(None) => break,
                            Err(e) => error!("Error reading stdout: {}", e),
                        }
                    }
                    result = stderr_reader.next_line() => {
                        match result {
                            Ok(Some(line)) => error!(stderr = %line),
                            Ok(None) => break,
                            Err(e) => error!("Error reading stderr: {}", e),
                        }
                    }
                    result = child.wait() => {
                        match result {
                            Ok(status) => {
                                if status.success() {
                                    return TaskCompleted::Success(now.elapsed());
                                } else {
                                    return TaskCompleted::Failed(now.elapsed());
                                }
                            },
                            Err(e) => {
                                error!("Error waiting for command: {}", e);
                                return TaskCompleted::Failed(now.elapsed());
                            }
                        }
                    }
                }
            }
        }
        return TaskCompleted::Skipped;
    }
}

#[derive(Debug)]
struct Tasks {
    roots: Vec<NodeIndex>,
    sender_tx: Sender<TaskUpdate>,
    graph: DiGraph<Arc<RwLock<TaskState>>, ()>,
    tasks_order: Vec<NodeIndex>,
}

impl Tasks {
    async fn new(config: Config) -> Result<(Self, Receiver<TaskUpdate>), Error> {
        let (sender_tx, receiver_rx) = channel(1000);
        let mut graph = DiGraph::new();
        let mut task_indices = HashMap::new();
        for task in config.tasks {
            let name = task.name.clone();
            if !task.name.contains(':')
                || task.name.split(':').count() != 2
                || task.name.starts_with(':')
                || task.name.ends_with(':')
                || !task
                    .name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == ':' || c == '_' || c == '-')
            {
                return Err(Error::InvalidTaskName(name));
            }
            let index = graph.add_node(Arc::new(RwLock::new(TaskState::new(task))));
            task_indices.insert(name, index);
        }
        let mut roots = Vec::new();
        for name in config.roots {
            if let Some(index) = task_indices.get(&name) {
                roots.push(*index);
            } else {
                return Err(Error::TaskNotFound(name));
            }
        }
        let mut tasks = Self {
            roots,
            sender_tx,
            graph,
            tasks_order: vec![],
        };
        tasks.resolve_dependencies(task_indices).await?;
        tasks.schedule().await?;
        Ok((tasks, receiver_rx))
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

    #[instrument(skip(self))]
    async fn schedule(&mut self) -> Result<(), Error> {
        // TODO: we traverse the graph twice, see https://github.com/petgraph/petgraph/issues/661
        let mut subgraph = DiGraph::new();

        // Map to track which nodes in the original graph correspond to which nodes in the new subgraph
        let mut node_map = HashMap::new();
        let mut visited = HashSet::new();

        // Traverse the graph starting from the root nodes
        for root_index in &self.roots {
            let mut dfs = Dfs::new(&self.graph, *root_index);

            while let Some(node) = dfs.next(&self.graph) {
                if visited.insert(node) {
                    // Add the node to the new subgraph and map it
                    let new_node = subgraph.add_node(self.graph[node].clone());
                    node_map.insert(node, new_node);

                    // Copy edges to the new subgraph
                    for edge in self.graph.edges(node) {
                        let target = edge.target();
                        if visited.contains(&target) {
                            // Both nodes must already be added to subgraph
                            let new_source = node_map[&node];
                            let new_target = node_map[&target];
                            subgraph.add_edge(new_source, new_target, ());
                        }
                    }
                }
            }
        }

        self.graph = subgraph;

        match toposort(&self.graph, None) {
            Ok(indexes) => {
                self.tasks_order = indexes;
                Ok(())
            }
            Err(cycle) => Err(Error::CycleDetected(
                self.graph[cycle.node_id()].read().await.task.name.clone(),
            )),
        }
    }

    #[instrument(skip(self))]
    async fn run(&mut self) -> Result<(), Error> {
        let mut running_tasks = JoinSet::new();

        for index in &self.tasks_order {
            let task_state = &self.graph[*index];

            loop {
                let mut dependencies_completed = true;
                for dep_index in self
                    .graph
                    .neighbors_directed(*index, petgraph::Direction::Outgoing)
                {
                    match &self.graph[dep_index].read().await.status {
                        TaskStatus::Completed(completed) => {
                            if completed.has_failed() {
                                let mut task_state = self.graph[dep_index].write().await;
                                task_state.status =
                                    TaskStatus::Completed(TaskCompleted::DependencyFailed);
                                continue;
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

                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }

            let task_state_clone = Arc::clone(task_state);

            running_tasks.spawn(async move {
                let mut task_state = task_state_clone.write().await;
                let completed = task_state.run().await;
                task_state.status = TaskStatus::Completed(completed);
            });
        }

        while let Some(res) = running_tasks.join_next().await {
            match res {
                Ok(_) => (),
                Err(e) => eprintln!("Task failed: {:?}", e),
            }
        }

        Ok(())
    }
}

struct TaskUpdate {
    name: String,
    status: TaskStatus,
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
    tasks: Arc<Mutex<Tasks>>,
    receiver_rx: Receiver<TaskUpdate>,
}

impl TasksUi {
    pub async fn new(config: Config) -> Result<Self, Error> {
        let (tasks, receiver_rx) = Tasks::new(config).await?;
        Ok(Self {
            tasks: Arc::new(Mutex::new(tasks)),
            receiver_rx,
        })
    }

    async fn get_tasks_status(&self) -> TasksStatus {
        let mut tasks_status = TasksStatus::new();
        let tasks = self.tasks.lock().await;

        for index in &tasks.tasks_order {
            let task_state = tasks.graph[*index].read().await;
            let (status_text, duration) = match &task_state.status {
                TaskStatus::Pending => {
                    tasks_status.pending += 1;
                    continue;
                }
                TaskStatus::Running(started) => {
                    tasks_status.running += 1;
                    ("Running".blue().bold(), Some(started.elapsed()))
                }
                TaskStatus::Completed(TaskCompleted::Skipped) => {
                    tasks_status.skipped += 1;
                    ("Skipped".blue().bold(), None)
                }
                TaskStatus::Completed(TaskCompleted::Success(duration)) => {
                    tasks_status.succeeded += 1;
                    ("Succeeded".green().bold(), Some(*duration))
                }
                TaskStatus::Completed(TaskCompleted::Failed(duration)) => {
                    tasks_status.failed += 1;
                    ("Failed".red().bold(), Some(*duration))
                }
                TaskStatus::Completed(TaskCompleted::DependencyFailed) => {
                    tasks_status.dependency_failed += 1;
                    ("Dependency failed".magenta().bold(), None)
                }
            };

            let duration = match duration {
                Some(d) => d.as_millis().to_string() + "ms",
                None => "".to_string(),
            };
            tasks_status.lines.push(format!(
                "{} {} {}",
                status_text, &task_state.task.name, duration
            ));
        }

        tasks_status
    }

    pub async fn run(&mut self) -> Result<TasksStatus, Error> {
        // start processing tasks
        let tasks_clone = Arc::clone(&self.tasks);
        let handle = tokio::spawn(async move {
            let mut tasks = tasks_clone.lock().await;
            if let Err(e) = tasks.run().await {
                eprintln!("Error running tasks: {:?}", e);
            }
        });

        // start TUI
        let mut stdout = io::stdout();
        let mut last_list_height: u16 = 0;

        loop {
            let tasks_status = self.get_tasks_status().await;

            execute!(
                stdout,
                // Clear the screen from the cursor down
                cursor::MoveUp(last_list_height),
                Clear(ClearType::FromCursorDown),
                style::PrintStyledContent(
                    format!(
                        "{}\nTasks: {}\n",
                        tasks_status.lines.join("\n"),
                        [
                            if tasks_status.pending > 0 {
                                format!("{} {}", "Pending".blue().bold(), tasks_status.pending)
                            } else {
                                String::new()
                            },
                            if tasks_status.running > 0 {
                                format!("{} {}", "Running".blue().bold(), tasks_status.running)
                            } else {
                                String::new()
                            },
                            if tasks_status.skipped > 0 {
                                format!("{} {}", "Skipped".blue().bold(), tasks_status.skipped)
                            } else {
                                String::new()
                            },
                            if tasks_status.succeeded > 0 {
                                format!("{} {}", "Succeeded".green().bold(), tasks_status.succeeded)
                            } else {
                                String::new()
                            },
                            if tasks_status.failed > 0 {
                                format!("{} {}", "Failed".red().bold(), tasks_status.failed)
                            } else {
                                String::new()
                            },
                            if tasks_status.dependency_failed > 0 {
                                format!(
                                    "{} {}",
                                    "Dependency Failed".red().bold(),
                                    tasks_status.dependency_failed
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
                )
            )?;

            last_list_height = tasks_status.lines.len() as u16 + 1;

            if handle.is_finished() {
                break;
            }

            // Sleep briefly to avoid excessive redraws
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

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
    let script1 =
        create_script("#!/bin/sh\necho 'Task 1 is running' && sleep 2 && echo 'Task 1 completed'")?;
    let script2 =
        create_script("#!/bin/sh\necho 'Task 2 is running' && sleep 3 && echo 'Task 2 completed'")?;
    let script3 =
        create_script("#!/bin/sh\necho 'Task 3 is running' && sleep 1 && echo 'Task 3 completed'")?;
    let script4 = create_script("#!/bin/sh\necho 'Task 4 is running' && echo 'Task 4 completed'")?;

    let (mut tasks, _) = Tasks::new(
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
    tasks.run().await?;

    // Assert the order is 1, 3, 4 and they all succeed
    assert_eq!(tasks.tasks_order.len(), 3);
    assert_matches!(
        tasks.graph[tasks.tasks_order[0]].read().await.status,
        TaskStatus::Completed(TaskCompleted::Success(_))
    );
    assert_eq!(
        tasks.graph[tasks.tasks_order[0]].read().await.task.name,
        "myapp:task_1"
    );
    assert_matches!(
        tasks.graph[tasks.tasks_order[1]].read().await.status,
        TaskStatus::Completed(TaskCompleted::Success(_))
    );
    assert_eq!(
        tasks.graph[tasks.tasks_order[1]].read().await.task.name,
        "myapp:task_3"
    );
    assert_matches!(
        tasks.graph[tasks.tasks_order[2]].read().await.status,
        TaskStatus::Completed(TaskCompleted::Success(_))
    );
    assert_eq!(
        tasks.graph[tasks.tasks_order[2]].read().await.task.name,
        "myapp:task_4"
    );
    Ok(())
}

// #[test(tokio::test)]
// async fn test_tasks_cycle() -> Result<(), Error> {
//     let (mut tasks, _) = Tasks::new(
//         Config::try_from(json!({
//             "roots": ["myapp:task_1"],
//             "tasks": [
//                 {
//                     "name": "myapp:task_1",
//                     "depends": ["myapp:task_2"],
//                     "command": "echo 'Task 1 is running' && sleep 2 && echo 'Task 1 completed'"
//                 },
//                 {
//                     "name": "myapp:task_2",
//                     "depends": ["myapp:task_1"],
//                     "command": "echo 'Task 2 is running' && sleep 3 && echo 'Task 2 completed'"
//                 }
//             ]
//         }))
//         .unwrap(),
//     )
//     .await?;

//     let err = "myapp_task_2".to_string();

//     assert!(matches!(tasks.run().await, Err(Error::CycleDetected(err))));
//     Ok(())
// }

#[test(tokio::test)]
async fn test_status() -> Result<(), Error> {
    let run_task = |root: &'static str| async move {
        let command_script1 =
            create_script("#!/bin/sh\necho 'Task 1 is running' && echo 'Task 1 completed'")?;
        let status_script1 = create_script("#!/bin/sh\nexit 0")?;
        let command_script2 =
            create_script("#!/bin/sh\necho 'Task 2 is running' && echo 'Task 2 completed'")?;
        let status_script2 = create_script("#!/bin/sh\nexit 1")?;

        Tasks::new(
            Config::try_from(json!({
                "roots": [root],
                "tasks": [
                    {
                        "name": "myapp:task_1",
                        "command": command_script1.to_str().unwrap(),
                        "status": status_script1.to_str().unwrap()
                    },
                    {
                        "name": "myapp:task_2",
                        "command": command_script2.to_str().unwrap(),
                        "status": status_script2.to_str().unwrap()
                    }
                ]
            }))
            .unwrap(),
        )
        .await
    };

    let (mut tasks, _) = run_task("myapp:task_1").await.unwrap();
    tasks.run().await?;
    assert_matches!(
        tasks.graph[tasks.tasks_order[0]].read().await.status,
        TaskStatus::Completed(TaskCompleted::Skipped)
    );

    let (mut tasks, _) = run_task("myapp:task_2").await.unwrap();
    tasks.run().await?;
    assert_matches!(
        tasks.graph[tasks.tasks_order[0]].read().await.status,
        TaskStatus::Completed(TaskCompleted::Success(_))
    );

    Ok(())
}

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
