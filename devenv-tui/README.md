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
use devenv_activity::{init, signal_done};
use devenv_tui::TuiApp;
use tokio_shutdown::Shutdown;

let (activity_rx, handle) = init();
handle.install();

let shutdown = Shutdown::new().expect("shutdown");

TuiApp::new(activity_rx, shutdown.clone())
    .run()
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
    .run()
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
- `Esc`: Clear selection

### Expanded Logs
- `↑/↓` or `j/k`: Scroll one line
- `PgUp/PgDn` or `Space`: Scroll page
- `g/G`: Jump to top/bottom
- `q/Esc`: Return to main view
