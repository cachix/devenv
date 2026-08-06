# devenv-tui

Terminal interface for devenv that displays build and evaluation activity.

## Architecture

```
devenv operations → ActivityEvent channel → TuiApp → Terminal
```

The TUI receives typed events from `devenv-activity` via a tokio mpsc channel and renders them using iocraft.

State is split into two parts:
- **ActivityModel**: Activities, logs, and messages (updated by event processor)
- **UiState**: Selection, scroll position, view mode (updated by input handlers)

Rendering is throttled to 30 FPS by default.

## Usage

```rust
use devenv_activity::init;
use devenv_tui::TuiApp;
use tokio_shutdown::Shutdown;

let (activity_rx, handle) = init();
handle.install();

let shutdown = Shutdown::new().expect("shutdown");

// Channel to signal TUI when backend work is done
let (backend_done_tx, backend_done_rx) = tokio::sync::oneshot::channel();

// When your backend work completes:
// let _ = backend_done_tx.send(());

TuiApp::new(activity_rx, shutdown.clone())
    .run(backend_done_rx)
    .await?;
```

### Configuration

```rust
TuiApp::new(activity_rx, shutdown)
    .batch_size(64)              // Events to batch before processing
    .max_messages(1000)          // Max standalone messages
    .max_build_logs(1000)        // Max log lines per build
    .collapsed_lines(10)         // Log lines in collapsed preview
    .max_fps(30)                 // Render rate limit
    .filter_level(ActivityLevel::Info)
    .run(backend_done_rx)
    .await?;
```

### Replay Tool

Replay captured traces for debugging:

```bash
# Capture traces
devenv --trace-output=file:trace.jsonl --trace-format json shell

# Compress traces on the fly
devenv --trace-output=file:>(zstd > trace.jsonl.zst) --trace-format json shell

# Replay in TUI
cargo run --bin tui-replay trace.jsonl

# Replay compressed traces
cargo run --bin tui-replay <(zstd -dc trace.jsonl.zst)
```

#### Reactive long-lived mode (for PTY tests and fuzzing)

`tui-replay` validates the complete JSONL trace before taking over the terminal
and rejects traces with no activity events. `--hold` keeps the TUI running after
the trace drains. `--reactive` connects TUI process commands to a deterministic
backend derived from the trace's process names and IDs; `--attached` enables the
real attach-mode interrupt prompt. Together these flags make a stable system
under test rather than a static recording:

```bash
cargo build -p devenv-tui --features deterministic-tui --bin tui-replay
bombadil terminal test --time-limit 20s --quiescence-timeout-ms 50 \
  --output-path /tmp/bombadil-out \
  --specification devenv-tui/bombadil/devenv-tui.spec.ts \
  -- ./target/debug/tui-replay --hold --attached --reactive \
    devenv-tui/replays/processes.jsonl
```

The trace is replayed once; repeating it would overwrite process states produced
by input and make the target non-causal. The fuzzer owns the PTY and injects
random input until its time limit. Reproduce a failing run with
`bombadil terminal test --reproduce /tmp/bombadil-out -- ...`.

For a deterministic regression of the same target, including resize, restart,
stop, and interrupt-prompt behavior, run:

```bash
cargo build -p devenv-run-tests -p devenv-tui \
  --features devenv-tui/deterministic-tui
bash devenv-tui/tests/replay-pty.sh \
  target/debug/devenv-run-tests target/debug/tui-replay
```

`--event-log PATH` writes and flushes semantic process commands and transitions
as JSONL. The PTY regression uses it to distinguish a real command round trip
from stale ANSI text in terminal history.

Note: use the pinned post-v0.6.1 Bombadil revision documented in
`bombadil/README.md`. Released v0.6.1 lacks PTY quiescence. The trace records
the full terminal grid per sampled state and grows quickly, so delete it after
triage.

## Views

**Main view**: Shows activity tree with inline log previews. Does not use alternate screen buffer, so terminal scrollback is preserved.

**Expanded logs**: Fullscreen view of logs for a single activity.

## Activity Types

| Type | Description |
|------|-------------|
| Build | Nix derivation builds with phase tracking and logs |
| Download | Store path downloads with byte progress |
| Query | Cache path queries |
| Tree | Flake input fetches |
| Evaluate | Nix evaluation |
| Task | Generic tasks |
| Command | Shell commands |
| Operation | devenv operations |
| Message | Standalone messages |

## Keyboard Shortcuts

### Main View
- `↑/↓` or `j/k`: Navigate activities
- `Ctrl+E`: Expand logs for selected activity
- `Ctrl+R`: Restart the selected managed process
- `Ctrl+C`: Show the quit prompt without stopping managed processes
- `c` or `Esc`: Keep running and clear the prompt
- `q` or `Ctrl+C`: Quit from the interrupt prompt
- `Esc`: Clear selection

### Expanded Logs
Long lines wrap onto continuation rows so their full content stays readable.

- `↑/↓` or `j/k`: Scroll one line
- `PgUp/PgDn` or `Space`: Scroll page
- `g/G`: Jump to top/bottom
- `Ctrl+C`: Copy the current selection, or show the quit prompt when no selection is active
- `c` or `Esc`: Keep running and clear the prompt
- `q` or `Ctrl+C`: Quit from the interrupt prompt
- `q/Esc`: Return to main view
