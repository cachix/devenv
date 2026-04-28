//! Console output for activity events when the TUI is disabled.
//!
//! Every event is normalized into a call to [`ConsoleOutput::begin`],
//! [`end`], [`log`], or [`message`]. Each of those funnels through
//! [`ConsoleOutput::write`] — the sole writer of stdout/stderr and the only
//! caller of [`ConsoleOutput::show_at`]. New event variants therefore cannot
//! regress the verbosity contract: there is no other way to emit a line.
//!
//! [`end`]: ConsoleOutput::end
//! [`log`]: ConsoleOutput::log
//! [`message`]: ConsoleOutput::message

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::io::{self, LineWriter, Write};
use std::sync::Arc;
use std::time::Instant;

use console::style;
use devenv_activity::{
    ActivityEvent, ActivityGuard, ActivityLevel, ActivityOutcome, Build, Command, Evaluate, Fetch,
    FetchKind, Operation, Process, Task,
};
use tokio::sync::{Notify, mpsc};

use crate::tasks::VerbosityLevel;
use crate::tracing::HumanReadableDuration;

struct Entry {
    name: String,
    start: Instant,
    level: ActivityLevel,
    /// Level at which child log lines from this activity render.
    log_level: ActivityLevel,
}

struct PendingTask {
    name: String,
    log_level: ActivityLevel,
}

#[derive(Copy, Clone)]
enum Sink {
    Stdout,
    Stderr,
}

pub struct ConsoleOutput {
    rx: mpsc::UnboundedReceiver<ActivityEvent>,
    verbosity: VerbosityLevel,
    entries: HashMap<u64, Entry>,
    /// Task names announced by `Task::Hierarchy` ahead of `Task::Start`. A task
    /// that never starts is dropped from this map without emitting anything.
    pending_tasks: HashMap<u64, PendingTask>,
    /// Display names of fetches we already announced, used to dedup duplicate
    /// Substitute/CopyPath pairs Nix emits for the same store path.
    active_fetch_names: HashSet<String>,
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
            pending_tasks: HashMap::new(),
            active_fetch_names: HashSet::new(),
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
        use ActivityLevel::{Debug, Info};
        match event {
            ActivityEvent::Build(Build::Start { id, name, .. }) => {
                self.begin(id, format!("Building {name}"), Info, Info);
            }
            ActivityEvent::Build(Build::Complete { id, outcome, .. }) => self.end(id, outcome),
            ActivityEvent::Build(Build::Log {
                id, line, is_error, ..
            }) => self.log(id, &line, is_error),
            ActivityEvent::Build(_) => {}

            ActivityEvent::Fetch(Fetch::Start { id, kind, name, .. }) => {
                // Nix emits Query alongside a Download for each .narinfo lookup
                // and a separate CopyPath Download that duplicates the
                // Substitute Download for the same store path.
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
                if !self.active_fetch_names.insert(display.clone()) {
                    return;
                }
                self.begin(id, display, Info, Info);
            }
            ActivityEvent::Fetch(Fetch::Complete { id, outcome, .. }) => {
                let Some(name) = self.entries.get(&id).map(|e| e.name.clone()) else {
                    return;
                };
                self.active_fetch_names.remove(&name);
                self.end(id, outcome);
            }
            ActivityEvent::Fetch(_) => {}

            ActivityEvent::Evaluate(Evaluate::Start {
                id, name, level, ..
            }) => self.begin(id, name, level, Info),
            ActivityEvent::Evaluate(Evaluate::Complete { id, outcome, .. }) => {
                self.end(id, outcome)
            }
            ActivityEvent::Evaluate(_) => {}

            // Tasks announce their full hierarchy before any start, so we
            // cache names here and emit `begin` lazily on `Task::Start`.
            ActivityEvent::Task(Task::Hierarchy { tasks, .. }) => {
                for t in tasks {
                    self.pending_tasks.insert(
                        t.id,
                        PendingTask {
                            name: format!("Running {}", t.name),
                            log_level: if t.show_output { Info } else { Debug },
                        },
                    );
                }
            }
            ActivityEvent::Task(Task::Start { id, .. }) => {
                if let Some(p) = self.pending_tasks.remove(&id) {
                    self.begin(id, p.name, Info, p.log_level);
                }
            }
            ActivityEvent::Task(Task::Complete { id, outcome, .. }) => self.end(id, outcome),
            ActivityEvent::Task(Task::Log {
                id, line, is_error, ..
            }) => self.log(id, &line, is_error),
            ActivityEvent::Task(Task::Progress { .. }) => {}

            // Commands wrap the actual shell invocation inside a task; the
            // parent task already prints a user-facing line, so the wrapper
            // sits at Debug. Logs from the wrapped command still flow at Info.
            ActivityEvent::Command(Command::Start { id, name, .. }) => {
                self.begin(id, name, Debug, Info);
            }
            ActivityEvent::Command(Command::Complete { id, outcome, .. }) => self.end(id, outcome),
            ActivityEvent::Command(Command::Log {
                id, line, is_error, ..
            }) => self.log(id, &line, is_error),

            ActivityEvent::Process(Process::Start {
                id, name, level, ..
            }) => self.begin(id, name, level, Info),
            ActivityEvent::Process(Process::Complete { id, outcome, .. }) => self.end(id, outcome),
            ActivityEvent::Process(Process::Log {
                id, line, is_error, ..
            }) => self.log(id, &line, is_error),
            ActivityEvent::Process(Process::Status { .. }) => {}

            ActivityEvent::Operation(Operation::Start {
                id, name, level, ..
            }) => self.begin(id, name, level, Info),
            ActivityEvent::Operation(Operation::Complete { id, outcome, .. }) => {
                self.end(id, outcome)
            }
            ActivityEvent::Operation(_) => {}

            ActivityEvent::Message(msg) => self.message(msg.level, &msg.text),
            ActivityEvent::SetExpected(_) | ActivityEvent::Shell(_) => {}
        }
    }

    fn begin(&mut self, id: u64, name: String, level: ActivityLevel, log_level: ActivityLevel) {
        let entry = Entry {
            name,
            start: Instant::now(),
            level,
            log_level,
        };
        self.write(
            level,
            Sink::Stderr,
            format_args!("{} {}", style("•").blue(), entry.name),
        );
        self.entries.insert(id, entry);
    }

    fn end(&mut self, id: u64, outcome: ActivityOutcome) {
        let Some(entry) = self.entries.remove(&id) else {
            return;
        };
        // Failures escalate to Error so they bubble through quiet, even if
        // the activity itself was at Debug (e.g. a Command wrapper).
        let level = if outcome.is_error() {
            ActivityLevel::Error
        } else {
            entry.level
        };
        let mark = match outcome {
            ActivityOutcome::Success => style("✓").green(),
            ActivityOutcome::Cached | ActivityOutcome::Skipped => style("✓").blue(),
            ActivityOutcome::Cancelled => style("•").yellow(),
            ActivityOutcome::Failed | ActivityOutcome::DependencyFailed => style("✖").red(),
        };
        let duration = HumanReadableDuration(entry.start.elapsed());
        self.write(
            level,
            Sink::Stderr,
            format_args!(
                "{mark} {} in {duration}{}",
                entry.name,
                outcome.display_suffix()
            ),
        );
    }

    fn log(&mut self, id: u64, line: &str, is_error: bool) {
        let level = if is_error {
            ActivityLevel::Error
        } else {
            self.entries
                .get(&id)
                .map(|e| e.log_level)
                .unwrap_or(ActivityLevel::Info)
        };
        let sink = if is_error { Sink::Stderr } else { Sink::Stdout };
        self.write(level, sink, format_args!("{line}"));
    }

    fn message(&mut self, level: ActivityLevel, text: &str) {
        let prefix = match level {
            ActivityLevel::Error => style("✖").red(),
            ActivityLevel::Warn => style("•").yellow(),
            _ => style("•").blue(),
        };
        self.write(level, Sink::Stderr, format_args!("{prefix} {text}"));
    }

    /// **The single output gate.** Every byte this module emits goes through
    /// here. The verbosity check lives nowhere else.
    fn write(&mut self, level: ActivityLevel, sink: Sink, args: fmt::Arguments) {
        if !self.show_at(level) {
            return;
        }
        let _ = match sink {
            Sink::Stdout => writeln!(self.stdout, "{args}"),
            Sink::Stderr => writeln!(self.stderr, "{args}"),
        };
    }

    fn show_at(&self, level: ActivityLevel) -> bool {
        match self.verbosity {
            VerbosityLevel::Quiet => level <= ActivityLevel::Error,
            VerbosityLevel::Normal => level <= ActivityLevel::Info,
            VerbosityLevel::Verbose => level <= ActivityLevel::Debug,
        }
    }
}

/// Guard returned by [`install`]. Drops the activity sender (closing the
/// channel), then joins the drain thread.
pub struct ConsoleGuard {
    activity: Option<ActivityGuard>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for ConsoleGuard {
    fn drop(&mut self) {
        drop(self.activity.take());
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

/// Render activity events to stderr until the returned guard is dropped.
pub fn install(verbosity: VerbosityLevel) -> ConsoleGuard {
    let (rx, handle) = devenv_activity::init();
    let activity = handle.install();
    let mut output = ConsoleOutput::new(rx, verbosity);
    let thread = std::thread::Builder::new()
        .name("devenv-console".into())
        .spawn(move || {
            while let Some(event) = output.rx.blocking_recv() {
                output.handle(event);
            }
        })
        .expect("spawn devenv-console thread");
    ConsoleGuard {
        activity: Some(activity),
        thread: Some(thread),
    }
}
