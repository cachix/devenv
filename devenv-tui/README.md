# devenv-tui

A Terminal User Interface (TUI) library that provides real-time visualization of devenv operations, including Nix builds, downloads, and evaluations.

## Overview

devenv-tui creates an interactive terminal interface that displays the progress of various development environment operations. It acts as a visualization layer between devenv's build processes and the user, transforming raw Nix logs and operation events into a comprehensible, real-time display.

## Adding replays

$ nix-build --log-format internal-json -vv -A chromium |& awk '{print strftime("%Y-%m-%dT%H:%M:%S%z"), $0}' > devenv-tui/replays/new
$ cargo run -p devenv-tui devenv-tui/replays/new

## How It Works

### Event Flow

1. **Event Generation**: Operations in devenv (builds, downloads, evaluations) generate events through the tracing framework or Nix's internal JSON logs
2. **Event Processing**: The TUI system receives these events through channels and updates its internal state
3. **State Management**: A centralized state manager tracks all active operations, their relationships, and progress
4. **Display Rendering**: The display layer continuously renders the current state to the terminal

### Key Components

**TuiState**: The central nervous system that maintains:
- Active operations and their hierarchical relationships
- Build progress and phase information
- Download statistics with speed calculations
- Log messages and build outputs
- Nix activity tracking (derivations, downloads, queries)

**Event System**: Messages flow through the system as typed events:
- Operation lifecycle (start/end)
- Progress updates
- Log messages
- Nix-specific activities

**Display Modes**:
- **Ratatui**: Full-featured TUI with scrollable views and keyboard navigation
- **Tui**: Simplified terminal interface
- **Console**: Plain text output for non-interactive environments

**Nix Integration**: A specialized bridge that:
- Parses Nix's internal JSON log format
- Translates Nix activities into TUI events
- Tracks build phases, download progress, and evaluations
- Maintains activity relationships and timings

### Visual Features

The TUI displays:
- **Operation Tree**: Hierarchical view of running operations
- **Progress Indicators**: Real-time progress bars for downloads and builds
- **Activity Summary**: Count of active builds, downloads, and queries
- **Build Phases**: Current phase for each build (unpacking, building, installing)
- **Download Speed**: Transfer rates and estimated completion times
- **Log Viewer**: Scrollable build logs for active derivations

### Terminal Management

The system uses modern terminal capabilities to:
- Create an inline viewport that doesn't clear existing terminal content
- Update specific regions without full screen redraws
- Handle terminal resize events gracefully
- Clean up properly on exit, preserving command history

## Use Cases

- **Interactive Development**: See what devenv is doing during shell activation
- **Build Monitoring**: Track parallel Nix builds and their dependencies
- **Download Progress**: Monitor package downloads from binary caches
- **Debugging**: Replay captured logs to diagnose issues
- **CI Integration**: Fallback to simple console output in non-TTY environments

## Architecture Benefits

- **Non-invasive**: Integrates through standard tracing infrastructure
- **Modular**: Display backends can be swapped based on terminal capabilities
- **Efficient**: Updates only changed portions of the display
- **Resilient**: Gracefully handles terminal issues and falls back to simpler modes

The tui-replay tool allows replaying captured process-compose logs with preserved timing, useful for debugging and demonstrations.
