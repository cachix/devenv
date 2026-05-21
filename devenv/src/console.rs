//! Console output for activity events when the TUI is disabled.
//!
//! Every event is normalized into a call to [`ConsoleOutput::begin`],
//! [`end`], [`log`], or [`message`]. Each of those funnels through
//! [`ConsoleOutput::write`] — the sole writer of stderr and the only
//! caller of [`ConsoleOutput::show_at`]. New event variants therefore cannot
//! regress the verbosity contract: there is no other way to emit a line.
//!
//! All output goes to stderr. stdout is owned by the caller's command
//! result (e.g. `devenv eval` JSON), so writing diagnostics there breaks
//! pipelines like `devenv eval … | jq`.
//!
//! [`end`]: ConsoleOutput::end
//! [`log`]: ConsoleOutput::log
//! [`message`]: ConsoleOutput::message

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::io::{self, LineWriter, Write};
use std::time::Instant;

/// Cap on per-entry suppressed log lines. Bounds memory for tasks that
/// emit megabytes of stdout; tail is preserved (most useful on failure).
const MAX_SUPPRESSED_LINES: usize = 1000;

/// Ring buffer that evicts the oldest entry on push when full.
struct BoundedLog {
    inner: VecDeque<String>,
    cap: usize,
}

impl BoundedLog {
    fn new(cap: usize) -> Self {
        Self {
            inner: VecDeque::with_capacity(cap),
            cap,
        }
    }

    fn push(&mut self, line: String) {
        if self.inner.len() == self.cap {
            self.inner.pop_front();
        }
        self.inner.push_back(line);
    }

    fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    fn iter(&self) -> impl Iterator<Item = &String> {
        self.inner.iter()
    }
}

use console::style;
use devenv_activity::{
    ActivityEvent, ActivityGuard, ActivityLevel, ActivityOutcome, Build, Command, Evaluate, Fetch,
    FetchKind, Operation, Process, Task,
};
use tokio::sync::{mpsc, oneshot};

use crate::tracing::HumanReadableDuration;
use devenv_core::VerbosityLevel;

struct Entry {
    name: String,
    start: Instant,
    level: ActivityLevel,
    /// Level at which child log lines from this activity render.
    log_level: ActivityLevel,
    /// Lines hidden by the verbosity gate. Replayed to stderr on failure
    /// so CI doesn't lose diagnostic output.
    suppressed_logs: BoundedLog,
}

struct PendingTask {
    name: String,
    log_level: ActivityLevel,
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
    // `new()` wraps this in `LineWriter` so writes coalesce per line
    // instead of re-locking stderr per fragment.
    stderr: Box<dyn Write + Send>,
}

impl ConsoleOutput {
    pub fn new(rx: mpsc::UnboundedReceiver<ActivityEvent>, verbosity: VerbosityLevel) -> Self {
        Self {
            rx,
            verbosity,
            entries: HashMap::new(),
            pending_tasks: HashMap::new(),
            active_fetch_names: HashSet::new(),
            stderr: Box::new(LineWriter::new(io::stderr())),
        }
    }

    /// Test-only: injects the writer, skipping `LineWriter` so tests can
    /// read raw buffers.
    #[cfg(test)]
    fn with_writer(
        rx: mpsc::UnboundedReceiver<ActivityEvent>,
        verbosity: VerbosityLevel,
        stderr: Box<dyn Write + Send>,
    ) -> Self {
        Self {
            rx,
            verbosity,
            entries: HashMap::new(),
            pending_tasks: HashMap::new(),
            active_fetch_names: HashSet::new(),
            stderr,
        }
    }

    /// Process events until the backend signals stop (sent or sender dropped),
    /// then drain any buffered events.
    pub async fn run(mut self, backend_done: oneshot::Receiver<()>) {
        let mut backend_done = backend_done;
        loop {
            tokio::select! {
                event = self.rx.recv() => match event {
                    Some(event) => self.handle(event),
                    None => break,
                },
                _ = &mut backend_done => {
                    while let Ok(event) = self.rx.try_recv() {
                        self.handle(event);
                    }
                    break;
                }
            }
        }
    }

    fn handle(&mut self, event: ActivityEvent) {
        use ActivityLevel::{Debug, Info, Trace};
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
            }) => self.begin(id, name, level, Debug),
            ActivityEvent::Evaluate(Evaluate::Complete { id, outcome, .. }) => {
                self.end(id, outcome)
            }
            ActivityEvent::Evaluate(Evaluate::Log { id, line, .. }) => self.log(id, &line, false),
            ActivityEvent::Evaluate(Evaluate::Op { id, op, .. }) => {
                self.log(id, &op.to_string(), false)
            }

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
            // start/end is hidden at Trace. Logs from the wrapped command
            // still flow at Info via the entry's log_level.
            ActivityEvent::Command(Command::Start { id, name, .. }) => {
                self.begin(id, name, Trace, Info);
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
            suppressed_logs: BoundedLog::new(MAX_SUPPRESSED_LINES),
        };
        self.write(level, format_args!("{} {}", style("•").blue(), entry.name));
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
        // On failure, dump anything the verbosity gate had hidden, since
        // these are now error context.
        if outcome.is_error() && !entry.suppressed_logs.is_empty() {
            for chunk in entry.suppressed_logs.iter() {
                self.write(ActivityLevel::Error, format_args!("  {chunk}"));
            }
        }
        let mark = match outcome {
            ActivityOutcome::Success => style("✓").green(),
            ActivityOutcome::Cached | ActivityOutcome::Skipped => style("✓").blue(),
            ActivityOutcome::Cancelled => style("•").yellow(),
            ActivityOutcome::Failed | ActivityOutcome::DependencyFailed => style("✖").red(),
        };
        let duration = HumanReadableDuration(entry.start.elapsed());
        self.write(
            level,
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
        let visible = self.show_at(level);
        // Indent so chunks nest visually under their activity's start line.
        for chunk in line.split('\n') {
            if visible {
                self.write(level, format_args!("  {chunk}"));
            } else if let Some(entry) = self.entries.get_mut(&id) {
                entry.suppressed_logs.push(chunk.to_string());
            }
        }
    }

    fn message(&mut self, level: ActivityLevel, text: &str) {
        let prefix = match level {
            ActivityLevel::Error => style("✖").red(),
            ActivityLevel::Warn => style("•").yellow(),
            _ => style("•").blue(),
        };
        self.write(level, format_args!("{prefix} {text}"));
    }

    /// **The single output gate.** Every byte this module emits goes through
    /// here. The verbosity check lives nowhere else.
    fn write(&mut self, level: ActivityLevel, args: fmt::Arguments) {
        if !self.show_at(level) {
            return;
        }
        let _ = writeln!(self.stderr, "{args}");
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

#[cfg(test)]
mod tests {
    use super::*;
    use devenv_activity::{TaskInfo, Timestamp};
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

    impl SharedBuffer {
        fn new() -> Self {
            Self(Arc::new(Mutex::new(Vec::new())))
        }

        fn contents(&self) -> String {
            // Strip ANSI so substring asserts don't fight with style codes.
            let bytes = self.0.lock().unwrap().clone();
            console::strip_ansi_codes(&String::from_utf8_lossy(&bytes)).into_owned()
        }

        fn into_box(self) -> Box<dyn Write + Send> {
            Box::new(BufferWriter(self.0))
        }
    }

    struct BufferWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for BufferWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct Harness {
        console: ConsoleOutput,
        stderr: SharedBuffer,
    }

    impl Harness {
        fn new(verbosity: VerbosityLevel) -> Self {
            let (_tx, rx) = mpsc::unbounded_channel::<ActivityEvent>();
            let stderr = SharedBuffer::new();
            let console = ConsoleOutput::with_writer(rx, verbosity, stderr.clone().into_box());
            Self { console, stderr }
        }

        fn dispatch(&mut self, event: ActivityEvent) {
            self.console.handle(event);
        }
    }

    fn task_hierarchy(id: u64, name: &str, show_output: bool) -> ActivityEvent {
        ActivityEvent::Task(Task::Hierarchy {
            tasks: vec![TaskInfo {
                id,
                name: name.to_string(),
                show_output,
                is_process: false,
            }],
            edges: vec![],
            timestamp: Timestamp::now(),
        })
    }

    fn task_start(id: u64) -> ActivityEvent {
        ActivityEvent::Task(Task::Start {
            id,
            timestamp: Timestamp::now(),
        })
    }

    fn task_log(id: u64, line: &str, is_error: bool) -> ActivityEvent {
        ActivityEvent::Task(Task::Log {
            id,
            line: line.to_string(),
            is_error,
            timestamp: Timestamp::now(),
        })
    }

    fn task_complete(id: u64, outcome: ActivityOutcome) -> ActivityEvent {
        ActivityEvent::Task(Task::Complete {
            id,
            outcome,
            timestamp: Timestamp::now(),
        })
    }

    /// Regression: `devenv test --no-tui` previously swallowed git-hook
    /// stdout because hidden-by-verbosity lines were dropped, not buffered.
    #[test]
    fn failing_task_replays_suppressed_stdout() {
        let mut h = Harness::new(VerbosityLevel::Normal);
        h.dispatch(task_hierarchy(1, "devenv:git-hooks:run", false));
        h.dispatch(task_start(1));
        h.dispatch(task_log(1, "shellcheck................Failed", false));
        h.dispatch(task_log(
            1,
            "In bad-script.sh line 1:\nfoo\n^-- SC2148",
            false,
        ));
        h.dispatch(task_complete(1, ActivityOutcome::Failed));

        let out = h.stderr.contents();
        assert!(
            out.contains("shellcheck................Failed"),
            "stdout line should be replayed on failure, got:\n{out}"
        );
        assert!(
            out.contains("SC2148"),
            "shellcheck diagnostic should be replayed on failure, got:\n{out}"
        );
        assert!(
            out.contains("✖ Running devenv:git-hooks:run"),
            "failure marker should appear, got:\n{out}"
        );
        assert!(out.contains("  foo"), "split chunk should appear in replay");
    }

    /// Suppressed stdout stays suppressed on success — healthy runs
    /// shouldn't flood the terminal with hook output.
    #[test]
    fn successful_task_does_not_replay_suppressed_stdout() {
        let mut h = Harness::new(VerbosityLevel::Normal);
        h.dispatch(task_hierarchy(1, "devenv:git-hooks:run", false));
        h.dispatch(task_start(1));
        h.dispatch(task_log(1, "all hooks passed", false));
        h.dispatch(task_complete(1, ActivityOutcome::Success));

        let out = h.stderr.contents();
        assert!(
            !out.contains("all hooks passed"),
            "stdout line should remain suppressed on success, got:\n{out}"
        );
        assert!(
            out.contains("✓ Running devenv:git-hooks:run"),
            "success marker should appear, got:\n{out}"
        );
    }

    /// `show_output: true` streams live; failure must not double-print.
    #[test]
    fn show_output_tasks_stream_live_no_duplicate_on_fail() {
        let mut h = Harness::new(VerbosityLevel::Normal);
        h.dispatch(task_hierarchy(1, "devenv:my:task", true));
        h.dispatch(task_start(1));
        h.dispatch(task_log(1, "step 1", false));
        h.dispatch(task_log(1, "step 2", false));
        h.dispatch(task_complete(1, ActivityOutcome::Failed));

        let stderr = h.stderr.contents();
        // Each line streamed exactly once.
        assert_eq!(
            stderr.matches("step 1").count(),
            1,
            "line should appear exactly once, got stderr:\n{stderr}"
        );
        assert_eq!(stderr.matches("step 2").count(), 1);
    }

    /// `is_error: true` lines are Error-level → always live; replay must
    /// not duplicate them.
    #[test]
    fn stderr_lines_shown_live_and_not_duplicated_on_fail() {
        let mut h = Harness::new(VerbosityLevel::Normal);
        h.dispatch(task_hierarchy(1, "devenv:my:task", false));
        h.dispatch(task_start(1));
        h.dispatch(task_log(1, "boom", true));
        h.dispatch(task_complete(1, ActivityOutcome::Failed));

        let stderr = h.stderr.contents();
        assert_eq!(
            stderr.matches("  boom").count(),
            1,
            "stderr line should appear exactly once, got:\n{stderr}"
        );
    }

    /// Quiet verbosity still replays on failure so CI logs aren't blind
    /// to the cause.
    #[test]
    fn quiet_verbosity_still_replays_on_failure() {
        let mut h = Harness::new(VerbosityLevel::Quiet);
        h.dispatch(task_hierarchy(1, "devenv:git-hooks:run", false));
        h.dispatch(task_start(1));
        h.dispatch(task_log(1, "SC2148 error", false));
        h.dispatch(task_complete(1, ActivityOutcome::Failed));

        let out = h.stderr.contents();
        assert!(
            out.contains("SC2148"),
            "suppressed line must replay on fail even at Quiet, got:\n{out}"
        );
    }

    /// Verbose shows all lines live (Debug passes the gate); replay must
    /// not double-print.
    #[test]
    fn verbose_streams_live_no_duplicate_on_fail() {
        let mut h = Harness::new(VerbosityLevel::Verbose);
        h.dispatch(task_hierarchy(1, "devenv:my:task", false));
        h.dispatch(task_start(1));
        h.dispatch(task_log(1, "trace line", false));
        h.dispatch(task_complete(1, ActivityOutcome::Failed));

        let stderr = h.stderr.contents();
        assert_eq!(
            stderr.matches("trace line").count(),
            1,
            "line should appear exactly once at Verbose, got stderr:\n{stderr}"
        );
    }

    /// `DependencyFailed` outcome also triggers replay.
    #[test]
    fn dependency_failed_replays_when_logs_present() {
        let mut h = Harness::new(VerbosityLevel::Normal);
        h.dispatch(task_hierarchy(1, "devenv:enterTest", false));
        h.dispatch(task_start(1));
        h.dispatch(task_log(1, "context line", false));
        h.dispatch(task_complete(1, ActivityOutcome::DependencyFailed));

        let out = h.stderr.contents();
        assert!(out.contains("context line"));
        assert!(out.contains("(dependency failed)"));
    }

    /// Buffer is capped; oldest lines evict so memory stays bounded
    /// even for tasks emitting megabytes of stdout. Tail (most useful
    /// on failure) survives.
    #[test]
    fn suppressed_logs_buffer_is_bounded() {
        let mut h = Harness::new(VerbosityLevel::Normal);
        h.dispatch(task_hierarchy(1, "devenv:my:task", false));
        h.dispatch(task_start(1));
        let overflow = MAX_SUPPRESSED_LINES + 50;
        for i in 0..overflow {
            h.dispatch(task_log(1, &format!("line-{i}"), false));
        }
        h.dispatch(task_complete(1, ActivityOutcome::Failed));

        let out = h.stderr.contents();
        // Tail preserved: last line present.
        assert!(
            out.contains(&format!("line-{}", overflow - 1)),
            "last line should survive, got:\n{out}"
        );
        // Head dropped: first 50 lines fell off the front of the ring.
        assert!(
            !out.contains("line-0\n") && !out.contains("line-49\n"),
            "oldest lines should be evicted, got:\n{out}"
        );
        // Replay count never exceeds the cap.
        let replayed = out.matches("line-").count();
        assert!(
            replayed <= MAX_SUPPRESSED_LINES,
            "replayed {replayed} lines, expected ≤ {MAX_SUPPRESSED_LINES}"
        );
    }
}
