use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use devenv_activity::{ActivityEvent, ActivityOutcome, Task as TaskEvent};
use tokio::sync::mpsc;

use crate::types::{TaskCompleted, TaskStatus, TasksStatus};
use crate::{Error, Outputs, Tasks, VerbosityLevel};

/// UI manager for tasks - consumes activity events and displays status
pub struct TasksUi {
    tasks: Arc<Tasks>,
    activity_rx: mpsc::UnboundedReceiver<ActivityEvent>,
    verbosity: VerbosityLevel,
    /// Track task states by activity ID
    task_states: HashMap<u64, TaskUiState>,
}

/// Internal state for tracking a task in the UI
struct TaskUiState {
    name: String,
    status: TaskDisplayStatus,
    start_time: Instant,
    show_output: bool,
}

/// Display status for a task
#[derive(Clone, PartialEq)]
enum TaskDisplayStatus {
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
        }
    }

    /// Run the UI, processing activity events until Done signal
    pub async fn run(
        mut self,
        run_handle: tokio::task::JoinHandle<Outputs>,
    ) -> Result<(TasksStatus, Outputs), Error> {
        // Print header (unless quiet mode)
        if self.verbosity != VerbosityLevel::Quiet {
            let names = console::style(self.tasks.root_names.join(", ")).bold();
            self.console_write_line(&format!("{:17} {}\n", "Running tasks", names))?;
        }

        // Process events until Done signal
        while let Some(event) = self.activity_rx.recv().await {
            match event {
                ActivityEvent::Task(task_event) => self.handle_task_event(task_event)?,
                ActivityEvent::Done => break,
                // Ignore other activity types (Build, Fetch, etc.)
                _ => {}
            }
        }

        // Wait for task runner to complete and get outputs
        let outputs = run_handle.await.map_err(|e| {
            Error::IoError(std::io::Error::other(format!("Task runner panicked: {e}")))
        })?;

        // Print summary
        self.print_summary().await?;

        // Get final status
        let status = self.tasks.get_completion_status().await;
        Ok((status, outputs))
    }

    fn handle_task_event(&mut self, event: TaskEvent) -> Result<(), Error> {
        match event {
            TaskEvent::Start {
                id,
                name,
                show_output,
                ..
            } => {
                self.task_states.insert(
                    id,
                    TaskUiState {
                        name: name.clone(),
                        status: TaskDisplayStatus::Running,
                        start_time: Instant::now(),
                        show_output,
                    },
                );

                if self.verbosity != VerbosityLevel::Quiet {
                    self.console_write_line(&format!(
                        "{:17} {}",
                        console::style("Running").blue().bold(),
                        console::style(&name).bold()
                    ))?;
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

                if let Some((name, status_label, duration_str, outcome)) = print_info {
                    if self.verbosity != VerbosityLevel::Quiet {
                        let style = match outcome {
                            ActivityOutcome::Success => console::style(status_label).green().bold(),
                            ActivityOutcome::Failed | ActivityOutcome::DependencyFailed => {
                                console::style(status_label).red().bold()
                            }
                            ActivityOutcome::Cancelled => {
                                console::style(status_label).yellow().bold()
                            }
                            ActivityOutcome::Cached | ActivityOutcome::Skipped => {
                                console::style(status_label).blue().bold()
                            }
                        };
                        self.console_write_line(&format!(
                            "{:17} {}{}",
                            style,
                            console::style(&name).bold(),
                            duration_str
                        ))?;
                    }
                }
            }
            TaskEvent::Log {
                id, line, is_error, ..
            } => {
                // Show log output based on verbosity and task's show_output setting
                // In quiet mode: no logs are shown
                // In verbose mode: all logs are shown
                // In normal mode: only show logs for tasks with show_output=true
                if let Some(state) = self.task_states.get(&id) {
                    let should_show = match self.verbosity {
                        VerbosityLevel::Quiet => false,
                        VerbosityLevel::Verbose => true,
                        VerbosityLevel::Normal => state.show_output,
                    };
                    if should_show {
                        let prefix = if is_error { "!" } else { " " };
                        self.console_write_line(&format!("[{}]{} {}", state.name, prefix, line))?;
                    }
                }
            }
            TaskEvent::Progress { .. } => {
                // Could show progress bar in future
            }
        }
        Ok(())
    }

    async fn print_summary(&self) -> Result<(), Error> {
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

            self.console_write_line(&status_summary)?;
        }

        // Print errors even in quiet mode
        let errors = self.format_task_errors().await;
        if !errors.is_empty() {
            let styled_errors = console::Style::new().apply_to(errors);
            self.console_write_line(&styled_errors.to_string())?;
        }

        Ok(())
    }

    fn console_write_line(&self, message: &str) -> Result<(), Error> {
        eprintln!("{}", message);
        Ok(())
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
