# devenv-tui

A Terminal User Interface (TUI) for devenv that provides real-time visualization of development environment operations through the tracing framework.

## Architecture

devenv-tui implements a tracing-based event system that captures structured logs from devenv operations and renders them in an interactive terminal interface:

```
devenv (library) ──tracing events──> devenv-tui ──iocraft──> Terminal
```

### Components

- **DevenvTuiLayer**: Tracing subscriber that captures structured events from devenv operations
- **TuiState**: Central state manager tracking active operations, progress, and logs
- **NixLogBridge**: Specialized parser for Nix's internal JSON log format
- **Event System**: Typed events for operation lifecycle, progress updates, and log messages

### Display Modes

- **Full TUI**: Interactive interface with scrollable views and keyboard navigation
- **Console**: Plain text output for non-interactive environments

## Usage

### As a Library

```rust
use devenv_tui::{init_tui};

// Initialize TUI system
let (tui_handle, rx) = init_tui();

// Register tracing layer
tracing_subscriber::registry()
    .with(tui_handle.layer)
    .init();

// Start TUI app
tokio::spawn(async move {
    devenv_tui::app::run(rx).await
});
```

### Replay Tool

Capture and replay Nix build logs for debugging:

```bash
# Capture logs
nix-build --log-format internal-json -vv -A chromium |& \
  awk '{print strftime("%Y-%m-%dT%H:%M:%S%z"), $0}' > replays/build.log

# Replay in TUI
cargo run --bin tui-replay replays/build.log
```

## Features

- **Real-time Progress**: Live progress bars for downloads and builds
- **Operation Tree**: Hierarchical view of running operations with dependencies
- **Build Phases**: Track current phase (unpacking, building, installing)
- **Download Monitoring**: Transfer rates and completion estimates
- **Log Streaming**: Scrollable build logs with structured output
- **Terminal Integration**: Inline viewport preserving command history

## Integration

devenv-tui integrates with devenv through:

1. **Tracing Events**: Captures structured logs via `tracing` spans and events
2. **Nix Bridge**: Parses Nix's `--log-format internal-json` output

The TUI system is designed to be the primary orchestrator for devenv commands, managing both execution and display formatting through a unified event stream.
