# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build and Test Commands

```bash
devenv shell --no-reload  # Enter development environment
cargo build               # Build the library
cargo nextest run         # Run unit tests
cargo nextest run --features test-all  # Run all tests (including PTY tests)
```

## Architecture

This is a Rust library for shell session management with hot-reload capability. When watched files change, it spawns a new shell in the background and seamlessly swaps to it, preserving terminal state using libghostty-rs (Ghostty VT terminal emulator).

### Module Overview

- **coordinator.rs** - `ShellCoordinator::run()` is the main entry point. Handles build coordination only; the TUI owns the PTY and terminal. Sends `ShellCommand`s to the TUI for spawn/swap; receives `ShellEvent`s for exit/resize.

- **builder.rs** - Defines `ShellBuilder` trait that consumers implement to provide shell commands. Receives `BuildContext` with cwd, env, and trigger (Initial vs FileChanged).

- **config.rs** - Configuration struct holding watch files list and debounce duration.

### Key Flow

1. Consumer implements `ShellBuilder` trait
2. `ShellCoordinator::run(config, builder, command_tx, event_rx)` runs initial build, then enters event loop forwarding builds to the TUI
3. File changes trigger rebuild via builder; coordinator sends new command to TUI which performs PTY swap + VT state replay
