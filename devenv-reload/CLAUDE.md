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

This is a Rust library for shell session management with hot-reload capability. When watched files change, it spawns a new shell in the background and seamlessly swaps to it, preserving terminal state using AVT (Abstract Virtual Terminal).

### Module Overview

- **manager.rs** - Core orchestrator. `ShellManager::run()` is the main entry point that:
  - Spawns initial shell via the builder
  - Sets up stdin/PTY/file-watcher event loop using tokio channels
  - Handles hot-reload: on file change, builds new shell, captures VT state, swaps PTYs, replays state
  - Manages raw terminal mode via `RawModeGuard`

- **builder.rs** - Defines `ShellBuilder` trait that consumers implement to provide shell commands. Receives `BuildContext` with cwd, env, and trigger (Initial vs FileChanged)

- **pty.rs** - PTY wrapper around `portable-pty`. Handles spawn, read/write, resize, kill

- **watcher.rs** - File watcher using `notify` crate with async channel output

- **config.rs** - Configuration struct holding watch files list and debounce duration

### Key Flow

1. Consumer implements `ShellBuilder` trait
2. `ShellManager::run(config, builder)` spawns shell and enters event loop
3. File changes trigger rebuild via builder, VT state capture, PTY swap
