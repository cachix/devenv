//! Console output for activity events when the TUI is disabled.
//!
//! Consumes the activity event channel and renders activity start/complete
//! lines plus task and process log forwarding to the terminal.

use std::collections::HashMap;
use std::io::{self, LineWriter, Write};
use std::sync::Arc;
use std::time::Instant;

use console::style;
use devenv_activity::{
    ActivityEvent, ActivityLevel, ActivityOutcome, Build, Command, Evaluate, Fetch, FetchKind,
    Operation, Process, Task,
};
use tokio::sync::{Notify, mpsc};

use crate::tasks::VerbosityLevel;
use crate::tracing::HumanReadableDuration;

struct Entry {
    name: String,
    start: Instant,
    show_log: bool,
}

pub struct ConsoleOutput {
    rx: mpsc::UnboundedReceiver<ActivityEvent>,
    verbosity: VerbosityLevel,
    entries: HashMap<u64, Entry>,
    // LineWriter coalesces format-arg writes into one syscall per line and
    // avoids re-acquiring the global stderr/stdout lock for every fragment.
    stdout: LineWriter<io::Stdout>,
    stderr: LineWriter<io::Stderr>,
}

impl ConsoleOutput {
    pub fn new(rx: mpsc::UnboundedReceiver<ActivityEvent>, verbosity: VerbosityLevel) -> Self {
        Self {
            rx,
            verbosity,
            entries: HashMap::new(),
            stdout: LineWriter::new(io::stdout()),
            stderr: LineWriter::new(io::stderr()),
        }
    }

    /// Process events until `shutdown` is notified, then drain any buffered events.
    pub async fn run(mut self, shutdown: Arc<Notify>) {
        loop {
            tokio::select! {
                event = self.rx.recv() => match event {
                    Some(event) => self.handle(event),
                    None => break,
                },
                _ = shutdown.notified() => {
                    while let Ok(event) = self.rx.try_recv() {
                        self.handle(event);
                    }
                    break;
                }
            }
        }
    }

    fn handle(&mut self, event: ActivityEvent) {
        match event {
            ActivityEvent::Build(Build::Start { id, name, .. }) => {
                self.start(id, format!("Building {name}"));
            }
            ActivityEvent::Build(Build::Complete { id, outcome, .. }) => self.complete(id, outcome),
            ActivityEvent::Build(Build::Log { line, is_error, .. }) => self.log(&line, is_error),
            ActivityEvent::Build(_) => {}

            ActivityEvent::Fetch(Fetch::Start { id, kind, name, .. }) => {
                // Drop noise: Nix emits Query alongside a Download for each .narinfo lookup
                // and emits a separate CopyPath Download that duplicates the Substitute
                // Download for the same store path. The Query and the .narinfo Download
                // pair to the same operation; the second same-named Download is the wrapper
                // around the file transfer.
                if kind == FetchKind::Query
                    || (kind == FetchKind::Download && name.ends_with(".narinfo"))
                {
                    return;
                }
                let display = match kind {
                    FetchKind::Download => format!("Downloading {name}"),
                    FetchKind::Tree => format!("Fetching {name}"),
                    FetchKind::Copy => format!("Copying {name}"),
                    FetchKind::Query => return,
                };
                if self.entry_name_active(&display) {
                    return;
                }
                self.start(id, display);
            }
            ActivityEvent::Fetch(Fetch::Complete { id, outcome, .. }) => self.complete(id, outcome),
            ActivityEvent::Fetch(_) => {}

            ActivityEvent::Evaluate(Evaluate::Start {
                id, name, level, ..
            }) => self.start_gated(id, name, level),
            ActivityEvent::Evaluate(Evaluate::Complete { id, outcome, .. }) => {
                self.complete(id, outcome)
            }
            ActivityEvent::Evaluate(_) => {}

            ActivityEvent::Task(Task::Hierarchy { tasks, .. }) => {
                for t in tasks {
                    self.entries.insert(
                        t.id,
                        Entry {
                            name: format!("Running {}", t.name),
                            start: Instant::now(),
                            show_log: t.show_output,
                        },
                    );
                }
            }
            ActivityEvent::Task(Task::Start { id, .. }) => {
                if !self.show_at(ActivityLevel::Info) {
                    return;
                }
                if let Some(e) = self.entries.get_mut(&id) {
                    e.start = Instant::now();
                    let _ = writeln!(self.stderr, "{} {}", style("•").blue(), e.name);
                }
            }
            ActivityEvent::Task(Task::Complete { id, outcome, .. }) => self.complete(id, outcome),
            ActivityEvent::Task(Task::Log {
                id, line, is_error, ..
            }) => {
                let show = match self.verbosity {
                    VerbosityLevel::Quiet => false,
                    VerbosityLevel::Verbose => true,
                    VerbosityLevel::Normal => self.entries.get(&id).is_some_and(|e| e.show_log),
                };
                if show {
                    let name = self
                        .entries
                        .get(&id)
                        .map(|e| e.name.strip_prefix("Running ").unwrap_or(&e.name))
                        .unwrap_or("?");
                    let prefix = if is_error { "!" } else { " " };
                    let _ = writeln!(self.stderr, "[{name}]{prefix} {line}");
                }
            }
            ActivityEvent::Task(Task::Progress { .. }) => {}

            // Command activities wrap the actual shell invocation inside a task — their
            // parent task already prints a user-facing line, so suppress the wrapper itself
            // unless verbose. Logs from the wrapped command still flow through.
            ActivityEvent::Command(Command::Start { id, name, .. }) => {
                if self.verbosity == VerbosityLevel::Verbose {
                    self.start(id, name);
                }
            }
            ActivityEvent::Command(Command::Complete { id, outcome, .. }) => {
                self.complete(id, outcome)
            }
            ActivityEvent::Command(Command::Log { line, is_error, .. }) => {
                self.log(&line, is_error)
            }

            ActivityEvent::Process(Process::Start {
                id, name, level, ..
            }) => {
                if self.show_at(level) {
                    let _ = writeln!(self.stderr, "{} {}", style("•").blue(), name);
                    self.entries.insert(
                        id,
                        Entry {
                            name,
                            start: Instant::now(),
                            show_log: true,
                        },
                    );
                }
            }
            ActivityEvent::Process(Process::Complete { id, outcome, .. }) => {
                self.complete(id, outcome)
            }
            ActivityEvent::Process(Process::Log { line, is_error, .. }) => {
                self.log(&line, is_error)
            }
            ActivityEvent::Process(Process::Status { .. }) => {}

            ActivityEvent::Operation(Operation::Start {
                id, name, level, ..
            }) => self.start_gated(id, name, level),
            ActivityEvent::Operation(Operation::Complete { id, outcome, .. }) => {
                self.complete(id, outcome)
            }
            ActivityEvent::Operation(_) => {}

            ActivityEvent::Message(msg) => {
                if self.show_at(msg.level) {
                    let prefix = match msg.level {
                        ActivityLevel::Error => style("✖").red(),
                        ActivityLevel::Warn => style("•").yellow(),
                        _ => style("•").blue(),
                    };
                    let _ = writeln!(self.stderr, "{prefix} {}", msg.text);
                }
            }
            ActivityEvent::SetExpected(_) | ActivityEvent::Shell(_) => {}
        }
    }

    fn show_at(&self, level: ActivityLevel) -> bool {
        match self.verbosity {
            VerbosityLevel::Quiet => level <= ActivityLevel::Error,
            VerbosityLevel::Normal => level <= ActivityLevel::Info,
            VerbosityLevel::Verbose => level <= ActivityLevel::Debug,
        }
    }

    fn entry_name_active(&self, name: &str) -> bool {
        self.entries.values().any(|e| e.name == name)
    }

    fn start(&mut self, id: u64, name: String) {
        if self.verbosity == VerbosityLevel::Quiet {
            return;
        }
        let _ = writeln!(self.stderr, "{} {}", style("•").blue(), name);
        self.entries.insert(
            id,
            Entry {
                name,
                start: Instant::now(),
                show_log: false,
            },
        );
    }

    fn start_gated(&mut self, id: u64, name: String, level: ActivityLevel) {
        if self.show_at(level) {
            self.start(id, name);
        }
    }

    fn complete(&mut self, id: u64, outcome: ActivityOutcome) {
        let Some(entry) = self.entries.remove(&id) else {
            return;
        };
        let mark = match outcome {
            ActivityOutcome::Success => style("✓").green(),
            ActivityOutcome::Cached | ActivityOutcome::Skipped => style("✓").blue(),
            ActivityOutcome::Cancelled => style("•").yellow(),
            ActivityOutcome::Failed | ActivityOutcome::DependencyFailed => style("✖").red(),
        };
        let duration = HumanReadableDuration(entry.start.elapsed());
        let _ = writeln!(
            self.stderr,
            "{mark} {} in {duration}{}",
            entry.name,
            outcome.display_suffix()
        );
    }

    fn log(&mut self, line: &str, is_error: bool) {
        if self.verbosity == VerbosityLevel::Quiet {
            return;
        }
        let _ = if is_error {
            writeln!(self.stderr, "{line}")
        } else {
            writeln!(self.stdout, "{line}")
        };
    }
}
