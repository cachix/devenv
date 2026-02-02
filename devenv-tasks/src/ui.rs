use std::collections::HashMap;
use std::io::{LineWriter, Write};
use std::sync::Arc;
use std::time::Instant;

use devenv_activity::{ActivityEvent, ActivityOutcome, Task as TaskEvent};
use tokio::sync::mpsc;

use crate::types::{TaskCompleted, TaskStatus, TasksStatus};
use crate::{Error, Outputs, Tasks, VerbosityLevel};

/// Line-buffered console output
struct Console {
    stdout: LineWriter<std::io::Stdout>,
    stderr: LineWriter<std::io::Stderr>,
}

impl Console {
    fn new() -> Self {
        Self {
            stdout: LineWriter::new(std::io::stdout()),
            stderr: LineWriter::new(std::io::stderr()),
        }
    }

    fn write_stdout(&mut self, message: &str) {
        let _ = writeln!(self.stdout, "{}", message);
    }

    fn write_stderr(&mut self, message: &str) {
        let _ = writeln!(self.stderr, "{}", message);
    }
}

/// UI manager for tasks - consumes activity events and displays status
pub struct TasksUi {
    tasks: Arc<Tasks>,
    activity_rx: mpsc::UnboundedReceiver<ActivityEvent>,
    verbosity: VerbosityLevel,
    /// Track task states by activity ID
    task_states: HashMap<u64, TaskUiState>,
    console: Console,
}

/// Internal state for tracking a task in the UI
struct TaskUiState {
    name: String,
    status: TaskDisplayStatus,
    start_time: Instant,
    show_output: bool,
    is_process: bool,
}

/// Display status for a task
#[derive(Clone, PartialEq)]
enum TaskDisplayStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Cached,
    Skipped,
    DependencyFailed,
}

impl TasksUi {
    pub fn new(
        tasks: Arc<Tasks>,
        activity_rx: mpsc::UnboundedReceiver<ActivityEvent>,
        verbosity: VerbosityLevel,
    ) -> Self {
        Self {
            tasks,
            activity_rx,
            verbosity,
            task_states: HashMap::new(),
            console: Console::new(),
        }
    }

    /// Run the UI, processing activity events until task runner completes
    pub async fn run(
        mut self,
        run_handle: tokio::task::JoinHandle<Outputs>,
    ) -> Result<(TasksStatus, Outputs), Error> {
        // Print header (unless quiet mode)
        if self.verbosity != VerbosityLevel::Quiet {
            let names = console::style(self.tasks.root_names.join(", ")).bold();
            self.console_write_stderr(&format!("{:17} {}\n", "Running tasks", names));
        }

        // Process events until task runner completes
        let mut run_handle = run_handle;
        let outputs = loop {
            tokio::select! {
                event = self.activity_rx.recv() => {
                    let Some(event) = event else {
                        // Channel closed unexpectedly - wait for run_handle
                        break run_handle.await.map_err(|e| {
                            Error::IoError(std::io::Error::other(format!("Task runner panicked: {e}")))
                        })?;
                    };
                    match event {
                        ActivityEvent::Task(task_event) => self.handle_task_event(task_event)?,
                        // Ignore other activity types (Build, Fetch, etc.)
                        _ => {}
                    }
                }
                result = &mut run_handle => {
                    // Task runner completed - drain any remaining events
                    while let Ok(event) = self.activity_rx.try_recv() {
                        if let ActivityEvent::Task(task_event) = event {
                            self.handle_task_event(task_event)?;
                        }
                    }
                    break result.map_err(|e| {
                        Error::IoError(std::io::Error::other(format!("Task runner panicked: {e}")))
                    })?;
                }
            }
        };

        // Print summary
        self.print_summary().await?;

        // Get final status
        let status = self.tasks.get_completion_status().await;
        Ok((status, outputs))
    }

    fn handle_task_event(&mut self, event: TaskEvent) -> Result<(), Error> {
        match event {
            TaskEvent::Hierarchy { tasks, .. } => {
                // Register all tasks upfront in Queued state
                for task_info in tasks {
                    self.task_states.insert(
                        task_info.id,
                        TaskUiState {
                            name: task_info.name,
                            status: TaskDisplayStatus::Queued,
                            start_time: Instant::now(),
                            show_output: task_info.show_output,
                            is_process: task_info.is_process,
                        },
                    );
                }
            }
            TaskEvent::Start { id, .. } => {
                // Transition from Queued to Running
                // Extract name first to avoid borrow checker issues
                let name = if let Some(state) = self.task_states.get_mut(&id) {
                    state.status = TaskDisplayStatus::Running;
                    state.start_time = Instant::now();
                    Some(state.name.clone())
                } else {
                    None
                };

                if let Some(name) = name {
                    if self.verbosity != VerbosityLevel::Quiet {
                        self.console_write_stderr(&format!(
                            "{:17} {}",
                            console::style("Running").blue().bold(),
                            console::style(&name).bold()
                        ));
                    }
                }
            }
            TaskEvent::Complete { id, outcome, .. } => {
                // Extract data from state first, then print (to avoid borrow checker issues)
                let print_info = if let Some(state) = self.task_states.get_mut(&id) {
                    let duration = state.start_time.elapsed();
                    let name = state.name.clone();

                    let (new_status, status_label, duration_str) = match outcome {
                        ActivityOutcome::Success => (
                            TaskDisplayStatus::Succeeded,
                            "Succeeded",
                            format!(" ({duration:.2?})"),
                        ),
                        ActivityOutcome::Failed => (
                            TaskDisplayStatus::Failed,
                            "Failed",
                            format!(" ({duration:.2?})"),
                        ),
                        ActivityOutcome::Cancelled => {
                            (TaskDisplayStatus::Cancelled, "Cancelled", String::new())
                        }
                        ActivityOutcome::Cached => {
                            (TaskDisplayStatus::Cached, "Cached", String::new())
                        }
                        ActivityOutcome::Skipped => {
                            (TaskDisplayStatus::Skipped, "No command", String::new())
                        }
                        ActivityOutcome::DependencyFailed => (
                            TaskDisplayStatus::DependencyFailed,
                            "Dependency failed",
                            String::new(),
                        ),
                    };

                    state.status = new_status;
                    Some((name, status_label, duration_str, outcome))
                } else {
                    None
                };

                if let Some((name, status_label, duration_str, outcome)) = print_info
                    && self.verbosity != VerbosityLevel::Quiet
                {
                    let style = match outcome {
                        ActivityOutcome::Success => console::style(status_label).green().bold(),
                        ActivityOutcome::Failed | ActivityOutcome::DependencyFailed => {
                            console::style(status_label).red().bold()
                        }
                        ActivityOutcome::Cancelled => console::style(status_label).yellow().bold(),
                        ActivityOutcome::Cached | ActivityOutcome::Skipped => {
                            console::style(status_label).blue().bold()
                        }
                    };
                    self.console_write_stderr(&format!(
                        "{:17} {}{}",
                        style,
                        console::style(&name).bold(),
                        duration_str
                    ));
                }
            }
            TaskEvent::Log {
                id, line, is_error, ..
            } => {
                if let Some(state) = self.task_states.get(&id) {
                    let should_show = state.is_process
                        || match self.verbosity {
                            VerbosityLevel::Quiet => false,
                            VerbosityLevel::Verbose => true,
                            VerbosityLevel::Normal => state.show_output,
                        };

                    if should_show {
                        if state.is_process {
                            if is_error {
                                self.console_write_stderr(&line);
                            } else {
                                self.console_write_stdout(&line);
                            }
                        } else {
                            let prefix = if is_error { "!" } else { " " };
                            self.console_write_stderr(&format!(
                                "[{}]{} {}",
                                state.name, prefix, line
                            ));
                        }
                    }
                }
            }
            TaskEvent::Progress { .. } => {
                // Could show progress bar in future
            }
        }
        Ok(())
    }

    async fn print_summary(&mut self) -> Result<(), Error> {
        let final_status = self.tasks.get_completion_status().await;

        if self.verbosity != VerbosityLevel::Quiet {
            let status_summary = [
                if final_status.pending > 0 {
                    format!(
                        "{} {}",
                        final_status.pending,
                        console::style("Pending").blue().bold()
                    )
                } else {
                    String::new()
                },
                if final_status.running > 0 {
                    format!(
                        "{} {}",
                        final_status.running,
                        console::style("Running").blue().bold()
                    )
                } else {
                    String::new()
                },
                if final_status.skipped > 0 {
                    format!(
                        "{} {}",
                        final_status.skipped,
                        console::style("Skipped").blue().bold()
                    )
                } else {
                    String::new()
                },
                if final_status.succeeded > 0 {
                    format!(
                        "{} {}",
                        final_status.succeeded,
                        console::style("Succeeded").green().bold()
                    )
                } else {
                    String::new()
                },
                if final_status.failed > 0 {
                    format!(
                        "{} {}",
                        final_status.failed,
                        console::style("Failed").red().bold()
                    )
                } else {
                    String::new()
                },
                if final_status.dependency_failed > 0 {
                    format!(
                        "{} {}",
                        final_status.dependency_failed,
                        console::style("Dependency Failed").red().bold()
                    )
                } else {
                    String::new()
                },
                if final_status.cancelled > 0 {
                    format!(
                        "{} {}",
                        final_status.cancelled,
                        console::style("Cancelled").yellow().bold()
                    )
                } else {
                    String::new()
                },
            ]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", ");

            self.console_write_stderr(&status_summary);
        }

        // Print errors even in quiet mode
        let errors = self.format_task_errors().await;
        if !errors.is_empty() {
            let styled_errors = console::Style::new().apply_to(errors);
            self.console_write_stderr(&styled_errors.to_string());
        }

        Ok(())
    }

    fn console_write_stdout(&mut self, message: &str) {
        self.console.write_stdout(message);
    }

    fn console_write_stderr(&mut self, message: &str) {
        self.console.write_stderr(message);
    }

    /// Format error messages from failed tasks
    async fn format_task_errors(&self) -> String {
        let mut errors = String::new();
        for index in &self.tasks.tasks_order {
            let task_state = self.tasks.graph[*index].read().await;
            if let TaskStatus::Completed(TaskCompleted::Failed(_, failure)) = &task_state.status {
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
        errors
    }
}
