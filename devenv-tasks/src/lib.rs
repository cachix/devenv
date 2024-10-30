use console::Term;
use miette::Diagnostic;
use petgraph::algo::toposort;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt::Display;
use std::process::Stdio;
use std::sync::Arc;
use thiserror::Error;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::{Notify, RwLock};
use tokio::task::JoinSet;
use tokio::time::{Duration, Instant};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    sync::Mutex,
};
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
    after: Vec<String>,
    #[serde(default)]
    before: Vec<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    inputs: Option<serde_json::Value>,
}

#[derive(Deserialize, Serialize)]
pub struct Config {
    pub tasks: Vec<TaskConfig>,
    pub roots: Vec<String>,
}

#[derive(Serialize)]
pub struct Outputs(BTreeMap<String, serde_json::Value>);
#[derive(Debug, Clone)]
pub struct Output(Option<serde_json::Value>);

impl TryFrom<serde_json::Value> for Config {
    type Error = serde_json::Error;

    fn try_from(json: serde_json::Value) -> Result<Self, Self::Error> {
        serde_json::from_value(json)
    }
}

type LinesOutput = Vec<(std::time::Instant, String)>;
impl std::ops::Deref for Outputs {
    type Target = BTreeMap<String, serde_json::Value>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone)]
struct TaskFailure {
    stdout: LinesOutput,
    stderr: LinesOutput,
    error: String,
}

#[derive(Debug, Clone)]
enum Skipped {
    Cached(Output),
    NotImplemented,
}

#[derive(Debug, Clone)]
enum TaskCompleted {
    Success(Duration, Output),
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

    fn prepare_command(
        &self,
        cmd: &str,
        outputs: &BTreeMap<String, serde_json::Value>,
    ) -> (Command, tempfile::NamedTempFile) {
        let mut command = Command::new(cmd);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());

        // Set DEVENV_TASK_INPUTS
        if let Some(inputs) = &self.task.inputs {
            command.env("DEVENV_TASK_INPUT", serde_json::to_string(inputs).unwrap());
        }

        // Create a temporary file for DEVENV_TASK_OUTPUT_FILE
        let outputs_file = tempfile::NamedTempFile::new().unwrap();
        command.env("DEVENV_TASK_OUTPUT_FILE", outputs_file.path());

        // Set environment variables from task outputs
        let mut devenv_env = String::new();
        for (_, value) in outputs.iter() {
            if let Some(env) = value.get("devenv").and_then(|d| d.get("env")) {
                if let Some(env_obj) = env.as_object() {
                    for (env_key, env_value) in env_obj {
                        if let Some(env_str) = env_value.as_str() {
                            command.env(env_key, env_str);
                            devenv_env.push_str(&format!(
                                "export {}={}\n",
                                env_key,
                                shell_escape(env_str)
                            ));
                        }
                    }
                }
            }
        }
        // Internal for now
        command.env("DEVENV_TASK_ENV", devenv_env);

        // Set DEVENV_TASKS_OUTPUTS
        let outputs_json = serde_json::to_string(outputs).unwrap();
        command.env("DEVENV_TASKS_OUTPUTS", outputs_json);

        (command, outputs_file)
    }

    async fn get_outputs(outputs_file: &tempfile::NamedTempFile) -> Output {
        let output = match File::open(outputs_file.path()).await {
            Ok(mut file) => {
                let mut contents = String::new();
                // TODO: report JSON parsing errors
                file.read_to_string(&mut contents).await.ok();
                serde_json::from_str(&contents).ok()
            }
            Err(_) => None,
        };
        Output(output)
    }

    #[instrument(ret)]
    async fn run(
        &self,
        now: Instant,
        outputs: &BTreeMap<String, serde_json::Value>,
    ) -> TaskCompleted {
        if let Some(cmd) = &self.task.status {
            let (mut command, outputs_file) = self.prepare_command(cmd, outputs);

            let result = command.status().await;
            match result {
                Ok(status) => {
                    if status.success() {
                        return TaskCompleted::Skipped(Skipped::Cached(
                            Self::get_outputs(&outputs_file).await,
                        ));
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
            let (mut command, outputs_file) = self.prepare_command(cmd, outputs);

            let result = command.spawn();

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
                                    return TaskCompleted::Success(now.elapsed(), Self::get_outputs(&outputs_file).await);
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
    notify_finished: Arc<Notify>,
    notify_ui: Arc<Notify>,
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
            notify_finished: Arc::new(Notify::new()),
            notify_ui: Arc::new(Notify::new()),
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

            for dep_name in &task_state.task.after {
                if let Some(dep_idx) = task_indices.get(dep_name) {
                    edges_to_add.push((*dep_idx, index));
                } else {
                    unresolved.insert((task_state.task.name.clone(), dep_name.clone()));
                }
            }

            for before_name in &task_state.task.before {
                if let Some(before_idx) = task_indices.get(before_name) {
                    edges_to_add.push((index, *before_idx));
                } else {
                    unresolved.insert((task_state.task.name.clone(), before_name.clone()));
                }
            }
        }

        for (from, to) in edges_to_add {
            self.graph.update_edge(from, to, ());
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
                for neighbor in self.graph.neighbors_undirected(node) {
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
    async fn run(&self) -> Outputs {
        let mut running_tasks = JoinSet::new();
        let outputs = Arc::new(Mutex::new(BTreeMap::new()));

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

                self.notify_finished.notified().await;
            }

            if dependency_failed {
                let mut task_state = task_state.write().await;
                task_state.status = TaskStatus::Completed(TaskCompleted::DependencyFailed);
                self.notify_finished.notify_one();
                self.notify_ui.notify_one();
            } else {
                let now = Instant::now();

                // hold write lock only to update the status
                {
                    let mut task_state = task_state.write().await;
                    task_state.status = TaskStatus::Running(now);
                }
                self.notify_ui.notify_one();

                let task_state_clone = Arc::clone(task_state);
                let outputs_clone = Arc::clone(&outputs);
                let notify_finished_clone = Arc::clone(&self.notify_finished);
                let notify_ui_clone = Arc::clone(&self.notify_ui);
                running_tasks.spawn(async move {
                    let completed = {
                        let outputs = outputs_clone.lock().await.clone();
                        task_state_clone.read().await.run(now, &outputs).await
                    };
                    {
                        let mut task_state = task_state_clone.write().await;
                        match &completed {
                            TaskCompleted::Success(_, Output(Some(output))) => {
                                outputs_clone
                                    .lock()
                                    .await
                                    .insert(task_state.task.name.clone(), output.clone());
                            }
                            TaskCompleted::Skipped(Skipped::Cached(Output(Some(output)))) => {
                                outputs_clone
                                    .lock()
                                    .await
                                    .insert(task_state.task.name.clone(), output.clone());
                            }
                            _ => {}
                        }
                        task_state.status = TaskStatus::Completed(completed);
                    }

                    notify_finished_clone.notify_one();
                    notify_ui_clone.notify_one();
                });
            }
        }

        while let Some(res) = running_tasks.join_next().await {
            match res {
                Ok(_) => (),
                Err(e) => error!("Task crashed: {}", e),
            }
        }

        self.notify_finished.notify_one();
        self.notify_ui.notify_one();
        Outputs(Arc::try_unwrap(outputs).unwrap().into_inner())
    }
}

#[derive(Debug)]
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
                        console::style(format!("{:17}", "Running")).blue().bold(),
                        Some(started.elapsed()),
                    )
                }
                TaskStatus::Completed(TaskCompleted::Skipped(skipped)) => {
                    tasks_status.skipped += 1;
                    let status = match skipped {
                        Skipped::Cached(_) => "Cached",
                        Skipped::NotImplemented => "Not implemented",
                    };
                    (console::style(format!("{:17}", status)).blue().bold(), None)
                }
                TaskStatus::Completed(TaskCompleted::Success(duration, _)) => {
                    tasks_status.succeeded += 1;
                    (
                        console::style(format!("{:17}", "Succeeded")).green().bold(),
                        Some(duration),
                    )
                }
                TaskStatus::Completed(TaskCompleted::Failed(duration, _)) => {
                    tasks_status.failed += 1;
                    (
                        console::style(format!("{:17}", "Failed")).red().bold(),
                        Some(duration),
                    )
                }
                TaskStatus::Completed(TaskCompleted::DependencyFailed) => {
                    tasks_status.dependency_failed += 1;
                    (
                        console::style(format!("{:17}", "Dependency failed"))
                            .magenta()
                            .bold(),
                        None,
                    )
                }
            };

            let duration = match duration {
                Some(d) => d.as_millis().to_string() + "ms",
                None => "".to_string(),
            };
            tasks_status.lines.push(format!(
                "{} {} {}",
                status_text,
                console::style(format!(
                    "{:width$}",
                    task_name,
                    width = self.tasks.longest_task_name
                ))
                .bold(),
                duration,
            ));
        }

        tasks_status
    }

    pub async fn run(&mut self) -> Result<(TasksStatus, Outputs), Error> {
        let names = console::style(self.tasks.root_names.join(", ")).bold();
        let term = Term::stderr();
        term.write_line(&format!("{:17} {}\n", "Running tasks", names))?;

        // start processing tasks
        let started = std::time::Instant::now();
        let tasks_clone = Arc::clone(&self.tasks);
        let handle = tokio::spawn(async move { tasks_clone.run().await });

        // start TUI
        let mut last_list_height: u16 = 0;

        loop {
            let tasks_status = self.get_tasks_status().await;

            let status_summary = [
                if tasks_status.pending > 0 {
                    format!(
                        "{} {}",
                        tasks_status.pending,
                        console::style("Pending").blue().bold()
                    )
                } else {
                    String::new()
                },
                if tasks_status.running > 0 {
                    format!(
                        "{} {}",
                        tasks_status.running,
                        console::style("Running").blue().bold()
                    )
                } else {
                    String::new()
                },
                if tasks_status.skipped > 0 {
                    format!(
                        "{} {}",
                        tasks_status.skipped,
                        console::style("Skipped").blue().bold()
                    )
                } else {
                    String::new()
                },
                if tasks_status.succeeded > 0 {
                    format!(
                        "{} {}",
                        tasks_status.succeeded,
                        console::style("Succeeded").green().bold()
                    )
                } else {
                    String::new()
                },
                if tasks_status.failed > 0 {
                    format!(
                        "{} {}",
                        tasks_status.failed,
                        console::style("Failed").red().bold()
                    )
                } else {
                    String::new()
                },
                if tasks_status.dependency_failed > 0 {
                    format!(
                        "{} {}",
                        tasks_status.dependency_failed,
                        console::style("Dependency Failed").red().bold()
                    )
                } else {
                    String::new()
                },
            ]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", ");

            let elapsed_time = format!("{:.2?}", started.elapsed());

            let output = format!(
                "{}\n{status_summary}{}{elapsed_time}",
                tasks_status.lines.join("\n"),
                " ".repeat(
                    (19 + self.tasks.longest_task_name)
                        .saturating_sub(console::measure_text_width(&status_summary))
                        .max(1)
                )
            );
            if !tasks_status.lines.is_empty() {
                let output = console::Style::new().apply_to(output);
                if last_list_height > 0 {
                    term.move_cursor_up(last_list_height as usize)?;
                    term.clear_to_end_of_screen()?;
                }
                term.write_line(&output.to_string())?;
            }

            if tasks_status.pending == 0 && tasks_status.running == 0 {
                break;
            }

            last_list_height = tasks_status.lines.len() as u16 + 1;

            self.tasks.notify_ui.notified().await;
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
            console::Style::new().apply_to(errors)
        };
        term.write_line(&errors.to_string())?;

        let tasks_status = self.get_tasks_status().await;
        Ok((tasks_status, handle.await.unwrap()))
    }
}

/// Escape a shell variable by wrapping it in single quotes.
/// Any single quotes within the variable are escaped.
fn shell_escape(s: &str) -> String {
    let mut escaped = String::with_capacity(s.len() + 2);
    escaped.push('\'');
    for c in s.chars() {
        match c {
            '\'' => escaped.push_str("'\\''"),
            _ => escaped.push(c),
        }
    }
    escaped.push('\'');
    escaped
}

#[cfg(test)]
mod test {
    use super::*;

    use pretty_assertions::assert_matches;
    use serde_json::json;
    use std::fs;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn test_shell_escape() {
        let escaped = shell_escape("foo'bar");
        eprintln!("{escaped}");
        assert_eq!(escaped, "'foo'\\''bar'");
    }

    #[tokio::test]
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

    #[tokio::test]
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
        let script4 =
            create_script("#!/bin/sh\necho 'Task 4 is running' && echo 'Task 4 completed'")?;

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
                        "after": ["myapp:task_1"],
                        "command": script3.to_str().unwrap()
                    },
                    {
                        "name": "myapp:task_4",
                        "after": ["myapp:task_3"],
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
                (name1, TaskStatus::Completed(TaskCompleted::Success(_, _))),
                (name2, TaskStatus::Completed(TaskCompleted::Success(_, _))),
                (name3, TaskStatus::Completed(TaskCompleted::Success(_, _)))
            ] if name1 == "myapp:task_1" && name2 == "myapp:task_3" && name3 == "myapp:task_4"
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_tasks_cycle() -> Result<(), Error> {
        let result = Tasks::new(
            Config::try_from(json!({
                "roots": ["myapp:task_1"],
                "tasks": [
                    {
                        "name": "myapp:task_1",
                        "after": ["myapp:task_2"],
                        "command": "echo 'Task 1 is running' && echo 'Task 1 completed'"
                    },
                    {
                        "name": "myapp:task_2",
                        "after": ["myapp:task_1"],
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

    #[tokio::test]
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
            TaskStatus::Completed(TaskCompleted::Skipped(Skipped::Cached(_)))
        );

        let tasks = create_tasks("myapp:task_2").await.unwrap();
        tasks.run().await;
        assert_eq!(tasks.tasks_order.len(), 1);
        assert_matches!(
            tasks.graph[tasks.tasks_order[0]].read().await.status,
            TaskStatus::Completed(TaskCompleted::Success(_, _))
        );

        Ok(())
    }

    #[tokio::test]
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

    #[tokio::test]
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

    #[tokio::test]
    async fn test_before_tasks() -> Result<(), Error> {
        let script1 = create_basic_script("1")?;
        let script2 = create_basic_script("2")?;
        let script3 = create_basic_script("3")?;

        let tasks = Tasks::new(
            Config::try_from(json!({
                "roots": ["myapp:task_1"],
                "tasks": [
                    {
                        "name": "myapp:task_1",
                        "command": script1.to_str().unwrap(),
                        "before": ["myapp:task_2", "myapp:task_3"]
                    },
                    {
                        "name": "myapp:task_2",
                        "before": ["myapp:task_3"],
                        "command": script2.to_str().unwrap()
                    },
                    {
                        "name": "myapp:task_3",
                        "command": script3.to_str().unwrap()
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
                (name1, TaskStatus::Completed(TaskCompleted::Success(_, _))),
                (name2, TaskStatus::Completed(TaskCompleted::Success(_, _))),
                (name3, TaskStatus::Completed(TaskCompleted::Success(_, _)))
            ] if name1 == "myapp:task_1" && name2 == "myapp:task_2" && name3 == "myapp:task_3"
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_after_tasks() -> Result<(), Error> {
        let script1 = create_basic_script("1")?;
        let script2 = create_basic_script("2")?;
        let script3 = create_basic_script("3")?;

        let tasks = Tasks::new(
            Config::try_from(json!({
                "roots": ["myapp:task_1"],
                "tasks": [
                    {
                        "name": "myapp:task_1",
                        "command": script1.to_str().unwrap(),
                        "after": ["myapp:task_3", "myapp:task_2"]
                    },
                    {
                        "name": "myapp:task_2",
                        "after": ["myapp:task_3"],
                        "command": script2.to_str().unwrap()
                    },
                    {
                        "name": "myapp:task_3",
                        "command": script3.to_str().unwrap()
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
                (name1, TaskStatus::Completed(TaskCompleted::Success(_, _))),
                (name2, TaskStatus::Completed(TaskCompleted::Success(_, _))),
                (name3, TaskStatus::Completed(TaskCompleted::Success(_, _)))
            ] if name1 == "myapp:task_3" && name2 == "myapp:task_2" && name3 == "myapp:task_1"
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_before_and_after_tasks() -> Result<(), Error> {
        let script1 = create_basic_script("1")?;
        let script2 = create_basic_script("2")?;
        let script3 = create_basic_script("3")?;

        let tasks = Tasks::new(
            Config::try_from(json!({
                "roots": ["myapp:task_1"],
                "tasks": [
                    {
                        "name": "myapp:task_1",
                        "command": script1.to_str().unwrap(),
                    },
                    {
                        "name": "myapp:task_3",
                        "after": ["myapp:task_1"],
                        "command": script3.to_str().unwrap()
                    },
                    {
                        "name": "myapp:task_2",
                        "before": ["myapp:task_3"],
                        "after": ["myapp:task_1"],
                        "command": script2.to_str().unwrap()
                    },
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
                (name1, TaskStatus::Completed(TaskCompleted::Success(_, _))),
                (name2, TaskStatus::Completed(TaskCompleted::Success(_, _))),
                (name3, TaskStatus::Completed(TaskCompleted::Success(_, _)))
            ] if name1 == "myapp:task_1" && name2 == "myapp:task_2" && name3 == "myapp:task_3"
        );
        Ok(())
    }

    // Test that tasks indirectly linked to the root are picked up and run.
    #[tokio::test]
    async fn test_transitive_dependencies() -> Result<(), Error> {
        let script1 = create_basic_script("1")?;
        let script2 = create_basic_script("2")?;
        let script3 = create_basic_script("3")?;

        let tasks = Tasks::new(
            Config::try_from(json!({
                "roots": ["myapp:task_3"],
                "tasks": [
                    {
                        "name": "myapp:task_1",
                        "command": script1.to_str().unwrap(),
                    },
                    {
                        "name": "myapp:task_2",
                        "after": ["myapp:task_1"],
                        "command": script2.to_str().unwrap()
                    },
                    {
                        "name": "myapp:task_3",
                        "after": ["myapp:task_2"],
                        "command": script3.to_str().unwrap()
                    },
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
                (name1, TaskStatus::Completed(TaskCompleted::Success(_, _))),
                (name2, TaskStatus::Completed(TaskCompleted::Success(_, _))),
                (name3, TaskStatus::Completed(TaskCompleted::Success(_, _)))
            ] if name1 == "myapp:task_1" && name2 == "myapp:task_2" && name3 == "myapp:task_3"
        );
        Ok(())
    }

    // Ensure that tasks before and after a root are run in the correct order.
    #[tokio::test]
    async fn test_non_root_before_and_after() -> Result<(), Error> {
        let script1 = create_basic_script("1")?;
        let script2 = create_basic_script("2")?;
        let script3 = create_basic_script("3")?;

        let tasks = Tasks::new(
            Config::try_from(json!({
                "roots": ["myapp:task_2"],
                "tasks": [
                    {
                        "name": "myapp:task_1",
                        "command": script1.to_str().unwrap(),
                        "before": [ "myapp:task_2"]
                    },
                    {
                        "name": "myapp:task_2",
                        "command": script2.to_str().unwrap()
                    },
                    {
                        "name": "myapp:task_3",
                        "after": ["myapp:task_2"],
                        "command": script3.to_str().unwrap()
                    },
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
                (name1, TaskStatus::Completed(TaskCompleted::Success(_, _))),
                (name2, TaskStatus::Completed(TaskCompleted::Success(_, _))),
                (name3, TaskStatus::Completed(TaskCompleted::Success(_, _)))
            ] if name1 == "myapp:task_1" && name2 == "myapp:task_2" && name3 == "myapp:task_3"
        );
        Ok(())
    }

    #[tokio::test]
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
                        "after": ["myapp:task_1"],
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

    #[tokio::test]
    async fn test_output_order() -> Result<(), Error> {
        let script1 = create_script(
            r#"#!/bin/sh
echo '{"key": "value1"}' > $DEVENV_TASK_OUTPUT_FILE
"#,
        )?;
        let script2 = create_script(
            r#"#!/bin/sh
echo '{"key": "value2"}' > $DEVENV_TASK_OUTPUT_FILE
"#,
        )?;
        let script3 = create_script(
            r#"#!/bin/sh
echo '{"key": "value3"}' > $DEVENV_TASK_OUTPUT_FILE
"#,
        )?;

        let tasks = Tasks::new(
            Config::try_from(json!({
                "roots": ["myapp:task_3"],
                "tasks": [
                    {
                        "name": "myapp:task_1",
                        "command": script1.to_str().unwrap(),
                    },
                    {
                        "name": "myapp:task_2",
                        "command": script2.to_str().unwrap(),
                        "after": ["myapp:task_1"],
                    },
                    {
                        "name": "myapp:task_3",
                        "command": script3.to_str().unwrap(),
                        "after": ["myapp:task_2"],
                    }
                ]
            }))
            .unwrap(),
        )
        .await?;

        let outputs = tasks.run().await;

        let keys: Vec<_> = outputs.keys().collect();
        assert_eq!(keys, vec!["myapp:task_1", "myapp:task_2", "myapp:task_3"]);

        Ok(())
    }

    #[tokio::test]
    async fn test_inputs_outputs() -> Result<(), Error> {
        let input_script = create_script(
            r#"#!/bin/sh
echo "{\"key\": \"value\"}" > $DEVENV_TASK_OUTPUT_FILE
if [ "$DEVENV_TASK_INPUT" != '{"test":"input"}' ]; then
    echo "Error: Input does not match expected value" >&2
    echo "Expected: $expected" >&2
    echo "Actual: $input" >&2
    exit 1
fi
"#,
        )?;

        let output_script = create_script(
            r#"#!/bin/sh
        if [ "$DEVENV_TASKS_OUTPUTS" != '{"myapp:task_1":{"key":"value"}}' ]; then
            echo "Error: Outputs do not match expected value" >&2
            echo "Expected: {\"myapp:task_1\":{\"key\":\"value\"}}" >&2
            echo "Actual: $DEVENV_TASKS_OUTPUTS" >&2
            exit 1
        fi
        echo "{\"result\": \"success\"}" > $DEVENV_TASK_OUTPUT_FILE
"#,
        )?;

        let tasks = Tasks::new(
            Config::try_from(json!({
                "roots": ["myapp:task_1", "myapp:task_2"],
                "tasks": [
                    {
                        "name": "myapp:task_1",
                        "command": input_script.to_str().unwrap(),
                        "inputs": {"test": "input"}
                    },
                    {
                        "name": "myapp:task_2",
                        "command": output_script.to_str().unwrap(),
                        "after": ["myapp:task_1"]
                    }
                ]
            }))
            .unwrap(),
        )
        .await?;

        let outputs = tasks.run().await;
        let task_statuses = inspect_tasks(&tasks).await;
        let task_statuses = task_statuses.as_slice();
        assert_matches!(
            task_statuses,
            [
                (name1, TaskStatus::Completed(TaskCompleted::Success(_, _))),
                (name2, TaskStatus::Completed(TaskCompleted::Success(_, _)))
            ] if name1 == "myapp:task_1" && name2 == "myapp:task_2"
        );

        assert_eq!(
            outputs.get("myapp:task_1").unwrap(),
            &json!({"key": "value"})
        );
        assert_eq!(
            outputs.get("myapp:task_2").unwrap(),
            &json!({"result": "success"})
        );

        Ok(())
    }

    async fn inspect_tasks(tasks: &Tasks) -> Vec<(String, TaskStatus)> {
        let mut result = Vec::new();
        for index in &tasks.tasks_order {
            let task_state = tasks.graph[*index].read().await;
            result.push((task_state.task.name.clone(), task_state.status.clone()));
        }
        result
    }

    fn create_script(script: &str) -> std::io::Result<tempfile::TempPath> {
        let mut temp_file = tempfile::Builder::new()
            .prefix("script")
            .suffix(".sh")
            .tempfile()?;
        temp_file.write_all(script.as_bytes())?;
        temp_file
            .as_file_mut()
            .set_permissions(fs::Permissions::from_mode(0o755))?;
        Ok(temp_file.into_temp_path())
    }

    fn create_basic_script(tag: &str) -> std::io::Result<tempfile::TempPath> {
        create_script(&format!(
            "#!/bin/sh\necho 'Task {tag} is running' && sleep 0.1 && echo 'Task {tag} completed'"
        ))
    }
}
