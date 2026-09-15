use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use clap::Parser;
use devenv_activity::{ActivityEvent, Process, ProcessStatus, Timestamp};
use devenv_mailbox::{FrontendCommand, FrontendEvent, ProcessCommand};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::signal;
use tokio::sync::mpsc;
use tokio::time::sleep;
use tokio_shutdown::Shutdown;
use tracing::info;

#[derive(Parser)]
#[command(about = "Replay devenv trace files with TUI visualization")]
struct Args {
    /// Path to the trace file (JSONL format).
    trace_file: PathBuf,

    /// Replay speed multiplier (e.g. 2.0 for 2x speed).
    #[arg(long, short, default_value = "1.0")]
    speed: f64,

    /// Keep the TUI running after the trace drains instead of exiting.
    #[arg(long)]
    hold: bool,

    /// Number of times to replay the trace (0 = repeat forever).
    #[arg(long = "loop", default_value = "1")]
    loop_count: u64,

    /// Render the interrupt prompt as an attached process session.
    #[arg(long)]
    attached: bool,

    #[arg(long, value_name = "PATH")]
    user_config: Option<PathBuf>,

    /// Respond deterministically to process restart/stop commands from the TUI.
    ///
    /// Process names and IDs are discovered from Process::Start events in the
    /// trace. This makes the replay a reactive system instead of a static film.
    #[arg(long)]
    reactive: bool,

    /// Write reactive commands and resulting states as flushed JSONL records.
    /// Intended for deterministic PTY assertions and fuzz failure diagnosis.
    #[arg(long, requires = "reactive")]
    event_log: Option<PathBuf>,
}

/// Raw tracing record as it appears in a JSONL trace.
#[derive(Debug, Deserialize)]
struct TraceEvent {
    target: String,
    timestamp: DateTime<Utc>,
    fields: serde_json::Value,
}

#[derive(Clone, Debug)]
struct ReplayEvent {
    timestamp: DateTime<Utc>,
    activity: ActivityEvent,
}

/// Parse and validate the complete trace before the TUI takes over the terminal.
///
/// Valid non-activity tracing records are ignored, but malformed JSON and
/// malformed `devenv_activity::events` records are errors. A trace with no
/// activity events is never a successful replay.
fn load_trace(reader: impl BufRead) -> Result<Vec<ReplayEvent>> {
    let mut events = Vec::new();

    for (index, line) in reader.lines().enumerate() {
        let line_num = index + 1;
        let line = line.with_context(|| format!("failed to read trace line {line_num}"))?;
        if line.trim().is_empty() {
            continue;
        }

        let mut trace: TraceEvent = serde_json::from_str(&line)
            .with_context(|| format!("invalid JSON trace record on line {line_num}"))?;
        if trace.target != "devenv_activity::events" {
            continue;
        }

        let value = trace
            .fields
            .get_mut("event")
            .map(serde_json::Value::take)
            .with_context(|| {
                format!("activity trace record on line {line_num} has no fields.event")
            })?;
        let activity = serde_json::from_value(value)
            .with_context(|| format!("invalid activity event on trace line {line_num}"))?;
        events.push(ReplayEvent {
            timestamp: trace.timestamp,
            activity,
        });
    }

    if events.is_empty() {
        bail!("trace contains no devenv activity events");
    }

    Ok(events)
}

fn load_trace_file(path: &Path) -> Result<Vec<ReplayEvent>> {
    let file = File::open(path)
        .with_context(|| format!("failed to open trace file: {}", path.display()))?;
    load_trace(BufReader::new(file))
        .with_context(|| format!("failed to load trace file: {}", path.display()))
}

async fn replay_events(
    events: &[ReplayEvent],
    tx: &mpsc::UnboundedSender<ActivityEvent>,
    speed: f64,
) -> Result<()> {
    let first_timestamp = events[0].timestamp;
    let start_time = Instant::now();

    for event in events {
        let time_offset = event.timestamp.signed_duration_since(first_timestamp);
        let target_elapsed_ms = time_offset.num_milliseconds().max(0) as f64 / speed;
        let target_elapsed = Duration::from_millis(target_elapsed_ms as u64);

        if target_elapsed > start_time.elapsed() {
            sleep(target_elapsed - start_time.elapsed()).await;
        }

        tx.send(event.activity.clone())
            .context("TUI closed while replaying activity events")?;
    }

    info!(activity_events = events.len(), "replay iteration finished");
    Ok(())
}

async fn run_replays(
    events: &[ReplayEvent],
    tx: &mpsc::UnboundedSender<ActivityEvent>,
    speed: f64,
    loop_count: u64,
) -> Result<()> {
    let mut iteration = 0;
    loop {
        replay_events(events, tx, speed).await?;
        iteration += 1;
        if loop_count != 0 && iteration >= loop_count {
            return Ok(());
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ReactiveProcess {
    id: u64,
    stable_status: ProcessStatus,
}

fn reactive_processes(events: &[ReplayEvent]) -> BTreeMap<String, ReactiveProcess> {
    events
        .iter()
        .filter_map(|event| match &event.activity {
            ActivityEvent::Process(Process::Start {
                id,
                name,
                ready_probe,
                ..
            }) => Some((
                name.clone(),
                ReactiveProcess {
                    id: *id,
                    stable_status: if ready_probe.is_some() {
                        ProcessStatus::Ready
                    } else {
                        ProcessStatus::Running
                    },
                },
            )),
            _ => None,
        })
        .collect()
}

fn same_process_command(left: &ProcessCommand, right: &ProcessCommand) -> bool {
    matches!(
        (left, right),
        (ProcessCommand::Restart(left), ProcessCommand::Restart(right))
            | (ProcessCommand::Stop(left), ProcessCommand::Stop(right))
            if left == right
    )
}

async fn recv_process_command(rx: &mut mpsc::Receiver<FrontendEvent>) -> Option<ProcessCommand> {
    while let Some(event) = rx.recv().await {
        if let FrontendEvent::Process(command) = event {
            return Some(command);
        }
    }
    None
}

fn latest_queued_process_command(rx: &mut mpsc::Receiver<FrontendEvent>) -> Option<ProcessCommand> {
    let mut latest = None;
    while let Ok(event) = rx.try_recv() {
        if let FrontendEvent::Process(command) = event {
            latest = Some(command);
        }
    }
    latest
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum HarnessRecord<'a> {
    Command {
        command: &'a str,
        process: &'a str,
    },
    Status {
        process: &'a str,
        status: ProcessStatus,
    },
}

fn record(file: &mut Option<File>, value: HarnessRecord<'_>) -> Result<()> {
    let Some(file) = file else {
        return Ok(());
    };
    serde_json::to_writer(&mut *file, &value).context("failed to write replay event log")?;
    writeln!(file).context("failed to terminate replay event log record")?;
    file.flush().context("failed to flush replay event log")
}

fn send_status(
    tx: &mpsc::UnboundedSender<ActivityEvent>,
    log: &mut Option<File>,
    process: &str,
    id: u64,
    status: ProcessStatus,
) -> Result<()> {
    record(log, HarnessRecord::Status { process, status })?;
    tx.send(ActivityEvent::Process(Process::Status {
        id,
        status,
        timestamp: Timestamp::now(),
    }))
    .context("TUI closed while applying a reactive process transition")
}

async fn run_reactive_backend(
    mut rx: mpsc::Receiver<FrontendEvent>,
    activity_tx: mpsc::UnboundedSender<ActivityEvent>,
    renderer_tx: mpsc::Sender<FrontendCommand>,
    processes: BTreeMap<String, ReactiveProcess>,
    mut log: Option<File>,
) -> Result<()> {
    let mut queued_command = None;
    while let Some(command) = match queued_command.take() {
        Some(command) => Some(command),
        None => recv_process_command(&mut rx).await,
    } {
        let handled_command = command.clone();

        match command {
            ProcessCommand::Restart(name) => {
                let process = processes
                    .get(&name)
                    .copied()
                    .with_context(|| format!("TUI requested unknown process {name:?}"))?;
                record(
                    &mut log,
                    HarnessRecord::Command {
                        command: "restart",
                        process: &name,
                    },
                )?;
                send_status(
                    &activity_tx,
                    &mut log,
                    &name,
                    process.id,
                    ProcessStatus::Restarting,
                )?;
                sleep(Duration::from_millis(100)).await;
                send_status(
                    &activity_tx,
                    &mut log,
                    &name,
                    process.id,
                    process.stable_status,
                )?;
            }
            ProcessCommand::Stop(name) => {
                let process = processes
                    .get(&name)
                    .copied()
                    .with_context(|| format!("TUI requested unknown process {name:?}"))?;
                record(
                    &mut log,
                    HarnessRecord::Command {
                        command: "stop",
                        process: &name,
                    },
                )?;
                send_status(
                    &activity_tx,
                    &mut log,
                    &name,
                    process.id,
                    ProcessStatus::Stopping,
                )?;
                sleep(Duration::from_millis(100)).await;
                send_status(
                    &activity_tx,
                    &mut log,
                    &name,
                    process.id,
                    ProcessStatus::Stopped,
                )?;
            }
            ProcessCommand::StopManager => {
                record(
                    &mut log,
                    HarnessRecord::Command {
                        command: "stop_manager",
                        process: "*",
                    },
                )?;
                for (name, process) in &processes {
                    send_status(
                        &activity_tx,
                        &mut log,
                        name,
                        process.id,
                        ProcessStatus::Stopping,
                    )?;
                }
                sleep(Duration::from_millis(100)).await;
                for (name, process) in &processes {
                    send_status(
                        &activity_tx,
                        &mut log,
                        name,
                        process.id,
                        ProcessStatus::Stopped,
                    )?;
                }
                // `FrontendCommand` carries a channel receiver, so its send
                // error is not `Sync` and cannot be wrapped with `context`.
                renderer_tx
                    .send(FrontendCommand::ExitRenderer)
                    .await
                    .map_err(|_| {
                        anyhow::anyhow!("TUI closed before process-manager shutdown completed")
                    })?;
                return Ok(());
            }
        };

        queued_command = latest_queued_process_command(&mut rx)
            .filter(|queued| !same_process_command(&handled_command, queued));
    }

    Ok(())
}

async fn ctrl_c() {
    signal::ctrl_c().await.expect("failed to listen for Ctrl+C");
}

async fn await_reactive_backend(
    task: &mut Option<tokio::task::JoinHandle<Result<()>>>,
) -> Result<()> {
    match task {
        Some(task) => task.await.context("reactive backend task panicked")?,
        None => std::future::pending().await,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    use tracing_subscriber::prelude::*;
    tracing_subscriber::registry().init();

    let args = Args::parse();
    if args.speed <= 0.0 || !args.speed.is_finite() {
        bail!("speed must be a finite number greater than 0");
    }

    // Validation deliberately happens before the TUI enters raw mode.
    let events = load_trace_file(&args.trace_file)?;
    let preferences = args
        .user_config
        .as_ref()
        .map(devenv_tui::UserConfig::load)
        .transpose()?
        .map(|config| config.tui);
    let processes = reactive_processes(&events);
    if args.reactive && processes.is_empty() {
        bail!("--reactive requires at least one process start event in the trace");
    }

    let event_log = args
        .event_log
        .as_ref()
        .map(|path| {
            OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(path)
                .with_context(|| format!("failed to create event log: {}", path.display()))
        })
        .transpose()?;

    let (activity_tx, activity_rx) = mpsc::unbounded_channel();
    let (renderer_tx, renderer_rx) = mpsc::channel(4);
    let (event_tx, event_rx) = mpsc::channel(16);
    let shutdown = Shutdown::new();

    if args.attached {
        renderer_tx
            .send(FrontendCommand::SetAttached(true))
            .await
            .map_err(|_| anyhow::anyhow!("failed to initialize attached replay mode"))?;
    }

    let mut app = devenv_tui::TuiApp::new(activity_rx, renderer_rx, shutdown.clone());
    if let Some(preferences) = preferences {
        app = app.with_preferences(preferences);
    }
    if args.reactive {
        app = app.with_event_sender(event_tx);
    } else {
        drop(event_tx);
    }

    info!(
        activity_events = events.len(),
        process_count = processes.len(),
        "spawning TUI"
    );
    let mut tui_task = tokio::spawn(async move { app.run().await.context("TUI failed") });

    let mut backend_task = args.reactive.then(|| {
        tokio::spawn(run_reactive_backend(
            event_rx,
            activity_tx.clone(),
            renderer_tx.clone(),
            processes,
            event_log,
        ))
    });

    let mut replay_task = tokio::spawn({
        let replay_tx = activity_tx.clone();
        let events = events.clone();
        let speed = args.speed;
        let loop_count = args.loop_count;
        async move { run_replays(&events, &replay_tx, speed, loop_count).await }
    });

    tokio::select! {
        result = &mut replay_task => {
            result.context("replay task panicked")??;

            if args.hold {
                info!("trace drained; holding TUI open");
                tokio::select! {
                    result = &mut tui_task => {
                        result.context("TUI task panicked")??;
                    }
                    result = await_reactive_backend(&mut backend_task) => {
                        result?;
                        backend_task = None;
                        tui_task.await.context("TUI task panicked")??;
                    }
                    _ = ctrl_c() => {
                        info!("interrupted");
                        shutdown.shutdown();
                    }
                }
            } else {
                renderer_tx
                    .send(FrontendCommand::ExitRenderer)
                    .await
                    .map_err(|_| anyhow::anyhow!("TUI closed before replay completion"))?;
                tui_task.await.context("TUI task panicked")??;
            }
        }
        result = &mut tui_task => {
            result.context("TUI task panicked")??;
        }
        result = await_reactive_backend(&mut backend_task) => {
            result?;
            backend_task = None;
            tui_task.await.context("TUI task panicked")??;
        }
        _ = ctrl_c() => {
            info!("interrupted");
            shutdown.shutdown();
            let _ = renderer_tx.send(FrontendCommand::ExitRenderer).await;
        }
    }

    if let Some(task) = backend_task.take() {
        task.abort();
    }
    replay_task.abort();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const PROCESS: &str = r#"{"target":"devenv_activity::events","timestamp":"2025-01-14T10:00:00Z","fields":{"event":{"activity_kind":"process","event":"start","id":7,"name":"web","parent":null,"command":"serve","ports":[{"name":"http","port":8080}],"ready_probe":{"kind":"http","host":"localhost","port":8080,"path":"/"},"level":"info","timestamp":"2025-01-14T10:00:00Z"}}}"#;

    #[test]
    fn rejects_non_json_trace() {
        let error = load_trace(Cursor::new("legacy text trace\n")).unwrap_err();
        assert!(error.to_string().contains("invalid JSON trace record"));
    }

    #[test]
    fn rejects_trace_without_activity_events() {
        let trace = r#"{"target":"other","timestamp":"2025-01-14T10:00:00Z","fields":{}}"#;
        let error = load_trace(Cursor::new(trace)).unwrap_err();
        assert!(error.to_string().contains("no devenv activity events"));
    }

    #[test]
    fn loads_processes_for_reactive_control() {
        let events = load_trace(Cursor::new(PROCESS)).unwrap();
        assert_eq!(
            reactive_processes(&events).get("web"),
            Some(&ReactiveProcess {
                id: 7,
                stable_status: ProcessStatus::Ready,
            })
        );
    }

    #[test]
    fn checked_in_process_fixture_is_replayable() {
        let events =
            load_trace(Cursor::new(include_str!("../../replays/processes.jsonl"))).unwrap();
        assert_eq!(events.len(), 9);
        let processes = reactive_processes(&events);
        assert_eq!(processes.len(), 3);
        assert_eq!(processes["api"].stable_status, ProcessStatus::Ready);
        assert_eq!(processes["worker"].stable_status, ProcessStatus::Running);
        assert_eq!(processes["disabled"].stable_status, ProcessStatus::Running);
    }

    #[test]
    fn rejects_legacy_renderer_control_as_an_activity() {
        let trace = r#"{"target":"devenv_activity::events","timestamp":"2025-01-14T10:00:00Z","fields":{"event":{"activity_kind":"control","control":"exit"}}}"#;
        let error = load_trace(Cursor::new(trace)).unwrap_err();
        assert!(error.to_string().contains("invalid activity event"));
    }

    #[tokio::test]
    async fn reactive_backend_applies_restart_and_stop_transitions() {
        let (event_tx, event_rx) = mpsc::channel(4);
        let (activity_tx, mut activity_rx) = mpsc::unbounded_channel();
        let (renderer_tx, _renderer_rx) = mpsc::channel(1);
        let task = tokio::spawn(run_reactive_backend(
            event_rx,
            activity_tx,
            renderer_tx,
            BTreeMap::from([(
                "web".to_string(),
                ReactiveProcess {
                    id: 7,
                    stable_status: ProcessStatus::Ready,
                },
            )]),
            None,
        ));

        event_tx
            .send(FrontendEvent::Process(ProcessCommand::Restart(
                "web".to_string(),
            )))
            .await
            .unwrap();
        assert_status(activity_rx.recv().await.unwrap(), ProcessStatus::Restarting);
        assert_status(activity_rx.recv().await.unwrap(), ProcessStatus::Ready);

        event_tx
            .send(FrontendEvent::Process(ProcessCommand::Stop(
                "web".to_string(),
            )))
            .await
            .unwrap();
        assert_status(activity_rx.recv().await.unwrap(), ProcessStatus::Stopping);
        assert_status(activity_rx.recv().await.unwrap(), ProcessStatus::Stopped);

        drop(event_tx);
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn reactive_backend_coalesces_restart_bursts_and_restores_running() {
        let (event_tx, event_rx) = mpsc::channel(64);
        for _ in 0..64 {
            event_tx
                .send(FrontendEvent::Process(ProcessCommand::Restart(
                    "web".to_string(),
                )))
                .await
                .unwrap();
        }
        drop(event_tx);

        let (activity_tx, mut activity_rx) = mpsc::unbounded_channel();
        let (renderer_tx, _renderer_rx) = mpsc::channel(1);
        let task = tokio::spawn(run_reactive_backend(
            event_rx,
            activity_tx,
            renderer_tx,
            BTreeMap::from([(
                "web".to_string(),
                ReactiveProcess {
                    id: 7,
                    stable_status: ProcessStatus::Running,
                },
            )]),
            None,
        ));

        assert_status(activity_rx.recv().await.unwrap(), ProcessStatus::Restarting);
        assert_status(activity_rx.recv().await.unwrap(), ProcessStatus::Running);
        task.await.unwrap().unwrap();
        assert!(activity_rx.recv().await.is_none());
    }

    #[tokio::test]
    async fn reactive_backend_task_errors_are_propagated() {
        let mut task = Some(tokio::spawn(async { bail!("backend failed") }));
        let error = await_reactive_backend(&mut task).await.unwrap_err();
        assert!(error.to_string().contains("backend failed"));
    }

    fn assert_status(event: ActivityEvent, expected: ProcessStatus) {
        assert!(matches!(
            event,
            ActivityEvent::Process(Process::Status { id: 7, status, .. }) if status == expected
        ));
    }
}
