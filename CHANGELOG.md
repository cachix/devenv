# Changelog

## X.Y.Z (YYYY-MM-DD)

### Bug Fixes

- Removed debug watchexec logs from showing up in `--no-reload` shells.
- Fixed TUI colors being unreadable on light terminal backgrounds.

### Improvements

### Breaking Changes

## 2.0.2 (2026-03-05)

### Bug Fixes

- Fixed `devenv test` not using the eval cache due to a temporary state directory being created on every run.
- Fixed TUI overflow being sent to scrollback instead of being clipped
- Fixed first Esc keypress being swallowed in shell reload mode ([#2548](https://github.com/cachix/devenv/issues/2548)).
- Fixed TUI displaying incorrect expected download count.
- Fixed TCP readiness probes only checking IPv4, causing hangs when processes bind to IPv6 loopback ([#2549](https://github.com/cachix/devenv/issues/2549)).

## 2.0.1 (2026-03-04)

### Bug Fixes

- Fixed TUI panic when navigating up/down with many processes ([#2542](https://github.com/cachix/devenv/issues/2542)).
- Fixed exec readiness probes not inheriting the process environment (e.g. `PGDATA`, `PGHOST`), causing probes to always fail.

### Improvements

- TUI now displays the readiness probe type when a process is in the starting state (e.g. `starting (exec: pg_isready)` or `starting (http: localhost:8080/health)`).

## 2.0.0 (2026-03-03)

This is a major release with significant architectural changes. devenv 2.0 introduces a native Rust process manager, a full terminal UI, hot-reload for shell environments, automatic port allocation, an eval caching layer, and many more improvements. See the [migration guide](https://devenv.sh/guides/migrating-to-2.0/) for detailed upgrade instructions.

### Breaking Changes

- **Native process manager is now the default.** The built-in Rust process manager replaces process-compose as the default for `devenv up`. If you rely on process-compose features, set `process.manager.implementation = "process-compose";` in your `devenv.nix`.
- **`devenv build` now returns JSON output** instead of plain store paths. Scripts that parse the output need to be updated, for example using `jq` to extract values.
- **`--log-format` CLI flag is removed.** Use `--trace-output` and `--trace-format` instead for controlling log output.
- **`devenv container --copy <name>` is removed.** Use the subcommand form `devenv container copy <name>` instead.
- **`devenv container --registry` option is removed** from `container run`.
- **`git-hooks` input is no longer included by default.** If you use `git-hooks.hooks`, add the input explicitly in your `devenv.yaml`.
- **The `devenv-generate` crate is removed** from the workspace.
- **`devenv test` no longer overrides `.devenv` by default.** The `.devenv` directory is preserved across test runs to allow eval cache to persist. Use `--override-dotfile` to opt-in to temporary directory isolation. The `--dont-override-dotfile` flag is kept as a hidden no-op for backward compatibility.

### New Features

#### Terminal UI (TUI)

- **New terminal UI is enabled by default.** devenv now displays a rich, interactive progress view for all operations. The TUI shows evaluation progress, build/download status, task execution, and process output in a structured tree view.
- Activity hierarchy with nested evaluation, build, and download tracking.
- Expandable log views for activities and processes (Ctrl+E to expand).
- Text selection and OSC 52 clipboard copy in expanded log view.
- Keyboard navigation with up/down arrows to select activities.
- Mouse scroll support in expanded view.
- Process output prefixed with `|` for clear visual separation.
- Running process count displayed in the status line.
- Millisecond-precision timestamps for completed activities.
- Completion checkmarks for finished activities.
- Automatically expanded logs for failed activities.
- The TUI is automatically disabled in CI environments and when trace output is set to stdout/stderr.
- `--tui` and `--no-tui` flags for explicit control.

#### Native Process Manager

- Built-in Rust process manager with full supervisor state machine.
- **Unified `ready` option** for processes and tasks:
  - `ready.exec` for command-based readiness checks.
  - `ready.http.get` for HTTP health probes.
  - `ready.tcp` for TCP port probes.
  - `ready.notify` for sd_notify (`READY=1`) protocol.
- **Unified `restart` option** with `on` (`"never"`, `"always"`, `"on_failure"`), `max` restart count, and `window` for rate limiting.
- Watchdog heartbeat and timeout extension support for the sd_notify protocol.
- `processes.<name>.linux.capabilities` option for granting specific Linux capabilities (e.g., `net_bind_service`).
- `processes.<name>.env` option for declarative per-process environment variables.
- `processes.<name>.cwd` option for setting the working directory.
- Startup timeout and rate limit configuration.
- File watching with configurable throttle for automatic process restarts.
- Process output streaming to TUI with tail display.
- Automatic `exec` wrapping for proper SIGTERM signal handling.

#### Automatic Port Allocation

- All service modules now use automatic port allocation by default, eliminating port conflicts between projects.
- Port allocations are cached and replayed through the eval cache for deterministic rebuilds.
- Allocated ports are displayed in the TUI alongside process activities.
- Strict mode rejects in-use ports during cache replay.

#### Eval Caching

- **Transparent caching for Nix evaluation results.** The eval cache stores and replays evaluation outputs, speeding up repeated `devenv shell` and `devenv up` invocations.
- File and environment dependency tracking for automatic cache invalidation.
- Support for caching resource allocations (ports) with replay validation.
- Detection of changes to unlocked inputs.
- Configurable via `--eval-cache` / `--no-eval-cache` flags.

#### Hot-Reload Shell

- **`devenv shell` now supports hot-reload.** When `devenv.nix` or tracked files change, the shell environment is automatically re-evaluated and updated without restarting the shell session.
- Status line showing reload progress with elapsed time and error toggle.
- Ctrl+Alt+D shortcut to pause/resume file watching.
- Direnv-style environment diffing for clean reloads.
- Scroll region support to keep the status line visible.

#### New Commands

- **`devenv eval <attribute>`**: Evaluate devenv.nix attributes and return results as JSON.
- **`devenv lsp`**: Launch the nixd language server with devenv-specific configuration for IDE integration. Provides hover, completion, and diagnostics for `devenv.nix` files.
- **`devenv tasks list`**: Lists tasks with entry points at the top level, matching the TUI hierarchy.

#### Task System Improvements

- **`--input` and `--input-json` CLI flags** for passing inputs to tasks.
- **`--refresh-task-cache` flag** to force task re-execution.
- **Soft dependencies with `@complete` suffix**: `after = [ "devenv:processes:cleanup@complete" ]` waits for exit regardless of success/failure.
- **Per-task `env` option** for declarative environment variables.
- `enterShell` tasks now run with TUI display before shell entry.
- `enterShell` task failures are non-fatal by default.
- Failed tasks are no longer cached when using `execIfModified`.
- Dynamic task name completion for bash, zsh, and fish shells.
- Task hierarchy visualization in the TUI via dependency edges.
- Tasks can run inside PTY shells for commands that require a terminal.
- Proper CWD validation before spawning tasks.

#### MCP (Model Context Protocol) Server

- HTTP transport support via `--http` flag for the MCP server.
- HTTP headers support for authentication.
- Package search now covers all packages instead of only "cachix".
- Default MCP server configured at `mcp.devenv.sh`.

#### Direnv Integration

- Simplified `.envrc` setup with single `devenv direnv-export` command.
- TUI support when running inside direnv.

### Improvements

- `--secretspec-provider` and `--secretspec-profile` CLI flags.
- Git revision shown in `devenv version` output.
- The Nix backend now uses C FFI bindings for direct API access, eliminating subprocess spawning.

### Bug Fixes

- Fixed process duplication when using `before`/`after` with process-compose.
- Fixed circular dependency in env vars with conditional processes (process-compose).
- Fixed infinite recursion in default settings for Kafka, Kafka Connect, and Keycloak services.
- Fixed terminal hang on Ctrl+C during shell building.
- Fixed missing prompt after task execution in shell.
- Fixed relative path inputs with `..` components.
- Fixed `devenv.local.nix` loading from imported directories.
- Fixed `devenv.cli.version` allowing null for flakes integration.
- Fixed `TMPDIR` separation from `DEVENV_RUNTIME` to avoid polluting `XDG_RUNTIME_DIR`.
- Fixed PATH preservation in shell and direnv environments.
- Fixed cursor position preservation after Ctrl+L clear in shell.
- Fixed eval cache handling of percent-encoded characters in database paths.
- Fixed changelog generation when assemble fails during `devenv update`.
- Fixed profile state isolation in separate directories.
- Fixed processes preventing infinite recursion when conditionally defining processes.
- Fixed load-exports permission error when file is owned by a different user.
- Fixed `-O packages:pkgs` replacing all packages instead of appending.
  The `pkgs` type now appends to existing packages by default.
  Use `pkgs!` to force-replace the entire list.
