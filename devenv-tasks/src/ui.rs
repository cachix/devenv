use std::sync::Arc;

use crate::types::{Skipped, TaskCompleted, TaskStatus, TasksStatus};
use crate::{Error, Outputs, Tasks, VerbosityLevel};

/// UI manager for tasks - simple status logging mode
pub struct TasksUi {
    tasks: Arc<Tasks>,
    verbosity: VerbosityLevel,
}

impl TasksUi {
    pub fn new(tasks: Tasks, verbosity: VerbosityLevel) -> TasksUi {
        TasksUi {
            tasks: Arc::new(tasks),
            verbosity,
        }
    }

    /// Count task statuses - simplified for non-TTY mode
    async fn get_tasks_status(&self) -> TasksStatus {
        let mut tasks_status = TasksStatus::new();

        for index in &self.tasks.tasks_order {
            let task_state = self.tasks.graph[*index].read().await;
            match task_state.status {
                TaskStatus::Pending => tasks_status.pending += 1,
                TaskStatus::ProcessReady => tasks_status.running += 1,
                TaskStatus::Running(_) => tasks_status.running += 1,
                TaskStatus::Completed(TaskCompleted::Skipped(_)) => tasks_status.skipped += 1,
                TaskStatus::Completed(TaskCompleted::Success(..)) => tasks_status.succeeded += 1,
                TaskStatus::Completed(TaskCompleted::Failed(..)) => tasks_status.failed += 1,
                TaskStatus::Completed(TaskCompleted::DependencyFailed) => {
                    tasks_status.dependency_failed += 1
                }
                TaskStatus::Completed(TaskCompleted::Cancelled(_)) => tasks_status.cancelled += 1,
            }
        }

        tasks_status
    }

    /// Run all tasks
    pub async fn run(&mut self) -> Result<(TasksStatus, Outputs), Error> {
        let tasks_clone = Arc::clone(&self.tasks);
        let handle = tokio::spawn(async move { tasks_clone.run().await });

        // If in quiet mode, just wait for tasks to complete
        if self.verbosity == VerbosityLevel::Quiet {
            loop {
                let tasks_status = self.get_tasks_status().await;
                if tasks_status.pending == 0 && tasks_status.running == 0 {
                    break;
                }
                self.tasks.notify_ui.notified().await;
            }

            // Print errors even in quiet mode
            let errors = self.format_task_errors().await;
            if !errors.is_empty() {
                let styled_errors = console::Style::new().apply_to(errors);
                self.console_write_line(&styled_errors.to_string())?;
            }

            let tasks_status = self.get_tasks_status().await;
            return Ok((tasks_status, handle.await.unwrap()));
        }

        let names = console::style(self.tasks.root_names.join(", ")).bold();

        // Always show which tasks are being run
        self.console_write_line(&format!("{:17} {}\n", "Running tasks", names))?;

        // start processing tasks
        let started = std::time::Instant::now();

        // Simple status logging mode (no cursor manipulation)
        let mut last_statuses = std::collections::HashMap::new();

        loop {
            let tasks_status = self.get_tasks_status().await;

            // Non-interactive mode - print only status changes
            for task_state in self.tasks.graph.node_weights() {
                let task_state = task_state.read().await;
                let task_name = &task_state.task.name;
                let current_status = match &task_state.status {
                    TaskStatus::Pending => "Pending".to_string(),
                    TaskStatus::ProcessReady => {
                        if let Some(previous) = last_statuses.get(task_name) {
                            if previous != "Ready" {
                                self.console_write_line(&format!(
                                    "{:17} {}",
                                    console::style("Ready").green().bold(),
                                    console::style(task_name).bold()
                                ))?;
                            }
                        } else {
                            self.console_write_line(&format!(
                                "{:17} {}",
                                console::style("Ready").green().bold(),
                                console::style(task_name).bold()
                            ))?;
                        }
                        "Ready".to_string()
                    }
                    TaskStatus::Running(_) => {
                        if let Some(previous) = last_statuses.get(task_name) {
                            if previous != "Running" {
                                self.console_write_line(&format!(
                                    "{:17} {}",
                                    console::style("Running").blue().bold(),
                                    console::style(task_name).bold()
                                ))?;
                            }
                        } else {
                            self.console_write_line(&format!(
                                "{:17} {}",
                                console::style("Running").blue().bold(),
                                console::style(task_name).bold()
                            ))?;
                        }
                        "Running".to_string()
                    }
                    TaskStatus::Completed(completed) => {
                        let (status, style, duration_str) = match completed {
                            TaskCompleted::Success(duration, _) => (
                                format!("Succeeded ({duration:.2?})"),
                                console::style("Succeeded").green().bold(),
                                format!(" ({duration:.2?})"),
                            ),
                            TaskCompleted::Skipped(Skipped::Cached(_)) => (
                                "Cached".to_string(),
                                console::style("Cached").blue().bold(),
                                "".to_string(),
                            ),
                            TaskCompleted::Skipped(Skipped::NoCommand) => (
                                "No command".to_string(),
                                console::style("No command").blue().bold(),
                                "".to_string(),
                            ),
                            TaskCompleted::Failed(duration, _) => (
                                format!("Failed ({duration:.2?})"),
                                console::style("Failed").red().bold(),
                                format!(" ({duration:.2?})"),
                            ),
                            TaskCompleted::DependencyFailed => (
                                "Dependency failed".to_string(),
                                console::style("Dependency failed").red().bold(),
                                "".to_string(),
                            ),
                            TaskCompleted::Cancelled(duration) => {
                                let duration_str =
                                    duration.map(|d| format!(" ({d:.2?})")).unwrap_or_default();
                                (
                                    format!("Cancelled{duration_str}"),
                                    console::style("Cancelled").yellow().bold(),
                                    duration_str,
                                )
                            }
                        };

                        if let Some(previous) = last_statuses.get(task_name) {
                            if previous != &status {
                                self.console_write_line(&format!(
                                    "{:17} {}{}",
                                    style,
                                    console::style(task_name).bold(),
                                    duration_str
                                ))?;
                            }
                        } else {
                            self.console_write_line(&format!(
                                "{:17} {}{}",
                                style,
                                console::style(task_name).bold(),
                                duration_str
                            ))?;
                        }
                        status
                    }
                };

                last_statuses.insert(task_name.clone(), current_status);
            }

            // Break early if there are no more tasks left
            if tasks_status.pending == 0 && tasks_status.running == 0 {
                break;
            }

            // Wait for task updates before looping
            self.tasks.notify_ui.notified().await;
        }

        // Calculate and print final summary after all tasks complete
        let final_status = self.get_tasks_status().await;
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

        let errors = self.format_task_errors().await;
        if !errors.is_empty() {
            let styled_errors = console::Style::new().apply_to(errors);
            self.console_write_line(&styled_errors.to_string())?;
        }

        let tasks_status = self.tasks.get_completion_status().await;
        Ok((tasks_status, handle.await.unwrap()))
    }

    fn console_write_line(&self, message: &str) -> std::io::Result<()> {
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
