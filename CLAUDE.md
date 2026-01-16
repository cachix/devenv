# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Development Commands

- **Build**: `cargo build`
- **Run CLI**: `cargo run -- [args]`
- **Build with Nix**: `nix build`
- **Format**: `cargo fmt`
- **Lint**: `cargo clippy`
- **Run all tests**: `devenv-run-tests run tests`
- **Run single test**: `devenv-run-tests run tests --only <test_name>`
- **Run unit tests**: `cargo test`
- **Run with nextest**: `cargo nextest run` (better process isolation)

## Architecture Overview

devenv is a Rust CLI tool that creates fast, declarative, reproducible developer environments using Nix. The codebase is organized as a Cargo workspace.

### Core Crates

- **devenv/** - Main CLI binary. Entry point is `main.rs` which handles three runtime modes (TUI, legacy CLI, tracing). Command dispatch happens in `devenv.rs`. CLI definitions use clap in `cli.rs`.

- **devenv-core/** - Shared types and abstractions:
  - `config.rs` - Configuration parsing (`devenv.yaml`, `devenv.local.yaml`)
  - `nix_backend.rs` - The `NixBackend` trait that abstracts Nix evaluation
  - `cli.rs` - `GlobalOptions` used across the codebase

- **devenv-nix-backend/** - C FFI-based Nix backend using `nix-bindings-*` crates. Provides direct API access to Nix without subprocess spawning. This is the default backend.

- **devenv-snix-backend/** - Experimental pure-Rust Nix evaluator backend using Snix (feature-gated with `snix`).

- **devenv-tasks/** - DAG-based task execution system with caching, parallel execution, and privilege escalation support.

- **devenv-activity/** - Tracing-based activity system that powers the TUI progress display. Use `#[activity("description")]` macro for TUI-visible operations.

- **devenv-tui/** - Terminal UI for displaying build progress and activities.

- **devenv-eval-cache/** - SQLite-based caching for Nix evaluation results. Tracks file and env dependencies to invalidate cache.

- **devenv-cache-core/** - Shared utilities for file hashing and SQLite operations used by both eval and task caches.

- **devenv-run-tests/** - Test harness for integration tests. Runs tests in isolated temp directories with fresh environments.

- **tokio-shutdown/** - Graceful shutdown manager handling SIGINT/SIGTERM with cleanup coordination.

- **nix-conf-parser/** - Parser for `nix.conf` format (output of `nix config show`).

- **xtask/** - Build automation (manpage and shell completion generation).

### Nix Modules

Nix modules in `src/modules/` define the devenv configuration schema:
- `languages/*.nix` - Language support (rust, python, go, etc.)
- `services/*.nix` - Service definitions (postgres, redis, etc.)
- `integrations/*.nix` - Tool integrations (git, starship, treefmt, etc.)
- `process-managers/*.nix` - Process management (process-compose, overmind, etc.)

### Configuration Flow

1. User creates `devenv.yaml` (inputs) and `devenv.nix` (configuration)
2. `Config::load()` in devenv-core parses YAML and resolves inputs
3. `Devenv::assemble()` generates a temporary flake structure in `.devenv/`
4. `NixBackend` evaluates the flake to produce shell environment or build outputs

### Key Patterns

- **Dual Backend Architecture**: The `NixBackend` trait allows swapping between the FFI-based backend (default) and Snix backend.
- **Activity Tracing**: Use `#[activity("description")]` macro or `Activity::operation()` for TUI-visible operations.
- **Error Handling**: Use `miette` for errors with `bail!()` and `?`. Custom error types use `thiserror`.
- **SQLite Migrations**: Both `devenv-eval-cache` and `devenv-tasks` use sqlx with migrations in `migrations/` directories.

## Testing

Integration tests live in `tests/` and `examples/` directories. Each test is a directory containing:
- `devenv.nix` - The configuration to test
- `.test.sh` - Test script (runs inside devenv shell by default)
- `.test-config.yml` (optional) - Test configuration:
  - `use_shell: false` - Run `.test.sh` directly, not in devenv shell
  - `git_init: false` - Don't initialize git repo in temp dir
  - `supported_systems` / `broken_systems` - Platform filtering

## Adding New CLI Subcommands

1. **Create implementation module** in `devenv/src/`:
```rust
// devenv/src/myfeature/mod.rs
pub async fn run(devenv: &crate::Devenv, args: Args) -> miette::Result<()> {
    Ok(())
}
```

2. **Add to CLI** in `devenv/src/cli.rs`:
```rust
#[derive(Subcommand, Clone)]
pub enum MyFeatureCommand {
    #[command(about = "Description.")]
    Action { ... },
}

// Add to Commands enum:
MyFeature {
    #[command(subcommand)]
    command: MyFeatureCommand,
},
```

3. **Wire up** in `devenv/src/main.rs` `run_devenv()` function.

## Code Style

- **Imports**: Group std lib, external crates, then internal
- **Naming**: `snake_case` for functions/variables, `CamelCase` for types
- **Error Handling**: Use `bail!()` not `panic!()`, propagate with `?`
- **No unsafe**: Don't use `unsafe` code

## Files That Should Not Be Edited

- `docs/reference/options.md` - Auto-generated from Nix module options
