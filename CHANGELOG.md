# Changelog

## X.Y.Z (unreleased)

### Bug Fixes

- Fixed eval cache storing inconsistent port allocations across different cached attributes ([#2631](https://github.com/cachix/devenv/issues/2631)).
- Fixed stale eval cache invalidation for `devenv up` process config changes caused by overlapping evaluations clearing each other's file dependency observers ([#2632](https://github.com/cachix/devenv/pull/2632)).
- Fixed child processes being left running on shutdown when using non-native process managers like process-compose ([#2586](https://github.com/cachix/devenv/issues/2586)).

### Improvements

- Extracted container, search, and gc methods from `devenv.rs` into separate submodule files for better code organization.

## 2.0.5 (2026-03-16)

### Improvements

- Show a quit prompt on first Ctrl+C in the TUI instead of immediately terminating ([#2607](https://github.com/cachix/devenv/pull/2607)).
- Patched Nix to avoid hitting GitHub rate limits when fetching flake inputs (upstreamed as [NixOS/nix#15470](https://github.com/NixOS/nix/pull/15470)).
- Improved eval performance by caching the initial Nix Value, avoiding re-evaluation of nixpkgs and the module system on subsequent attribute lookups (~2x time-to-shell improvement).
- `devenv-run-tests`: `--only` and `--exclude` now support glob patterns (e.g. `--only 'python-*'`).

### Bug Fixes

- Fixed `devenv test` not running `enterTest` tasks (e.g., `git-hooks:run`) in devenv 2.0+. Also: `devenv test` now fails early when enterTest tasks fail, and skips redundant `load_tasks()` when tasks are already handled.
- Fixed file watcher dropping change events during the initial bootstrap file flood by switching from `try_send` to backpressure, which caused `devenv.nix` changes to go undetected during hot reload.
- Fixed `exec_if_modified` performance when negation patterns were used, avoiding a full walk of the parent directory for literal file paths.
- Fixed child processes (postgres, redis, etc.) being left running after `devenv up` exits or `devenv processes down` is called. The native manager wrapper now forwards TERM/INT signals to the child process group, and the process-compose backend creates a proper process group for signaling ([#2619](https://github.com/cachix/devenv/issues/2619)).
- Fixed secretspec prompting for secrets in non-interactive contexts like direnv.
- Fixed `devenv search` showing truncated package names (e.g. `pkgs.` instead of `pkgs.ncdu`).
- Fixed runtime directory path (`devenv-<hash>`) being inconsistent on macOS when paths contain symlinks (e.g. `/tmp` vs `/private/tmp`), which could cause processes to look for sockets in the wrong directory.
- Fixed TUI hanging when the backend encounters an error in the PTY shell path (e.g. Nix evaluation failure).
- Fixed `nix run` trying to run `devenv-wrapped` which doesn't exist.
- Fixed in-band resize events being sent to the shell when the app did not opt-in to receiving them.
- Fixed a packaging error in nixpkgs that resulted in the macOS builds of devenv to include two conflicting copies of the Boehm GC (#2552, #2576).
- Fixed `devenv init`, `devenv test`, and secretspec hint messages being silently dropped due to missing user-message marker.

## 2.0.4 (2026-03-11)

### Bug Fixes

- Fixed `files` and other task-based features not working via direnv in devenv v2 because `devenv direnv-export` did not run enterShell tasks ([#2602](https://github.com/cachix/devenv/issues/2602)).
- Fixed `devenv shell` hanging in certain CI environments by requiring both stdin and stdout to be a real terminal before launching the PTY reload shell ([#2597](https://github.com/cachix/devenv/issues/2597)).
- Added a timeout to the terminal cursor position query to prevent hangs in PTY environments that don't respond to DSR queries.
- Fixed process dependencies not being respected in the native process manager ([#2554](https://github.com/cachix/devenv/issues/2554)).
- Fixed `execIfModified` task cache not invalidating when a previously watched file is deleted, renamed, or moved outside the glob pattern ([#2577](https://github.com/cachix/devenv/issues/2577)).
- Fixed task exports (e.g. `VIRTUAL_ENV`, `PATH` from venv) not being set in the reload shell.
- Fixed port allocation not detecting ports bound to `0.0.0.0` or `[::]`, causing multiple devenv instances to allocate the same port ([#2567](https://github.com/cachix/devenv/issues/2567)).
- Fixed cursor position requests not being passed through in the shell ([#2570](https://github.com/cachix/devenv/issues/2570)).
- Fixed "Threads explicit registering is not previously enabled" crash on some Nix versions by calling `GC_allow_register_threads()` after `libexpr_init()` ([#2576](https://github.com/cachix/devenv/issues/2576)).
- Fixed `-O packages:pkgs` causing infinite recursion by using NixOS module system merging instead of self-referencing `config.packages`.
- Fixed `--clean` shell environment capture keeping all host variables except the keep list (inverted filter).
- Fixed `devenv search` output being drawn directly to stdout over the TUI.
- Fixed the shell not forwarding several terminal escape sequences (DA2, DA3, XTVERSION, DCS, DECRQM, Kitty keyboard protocol, XTMODIFYOTHERKEYS, mouse modes, and color scheme reporting), which broke TUI programs like neovim and helix.
- Fixed the shell not cleaning up Kitty keyboard protocol and XTMODIFYOTHERKEYS state on exit, which left the terminal in a broken state after a program crash.
- Fixed the shell not sending in-band resize notifications (mode 2048) through the PTY, which caused programs that rely on this protocol to miss resize events.
- Fixed the shell forwarding text area size queries (CSI 18 t) to the real terminal, which returned incorrect dimensions that included the status line row.
- Fixed the devenv main thread and REPL thread using the default stack size instead of 64MB, which could cause stack overflows during deep Nix evaluations.
- Fixed TUI sometimes overwriting the shell prompt/command line after commands like `devenv update`, caused by cancelling iocraft's render loop mid-frame.

### Improvements

- Added `strictPorts` option to `devenv.yaml` for configuring strict port mode as a project default, along with `--no-strict-ports` CLI flag to override it ([#2606](https://github.com/cachix/devenv/issues/2606)).
- Bumped secretspec to 0.8.0 and enabled all provider features (Google Cloud Secret Manager, AWS Secrets Manager, HashiCorp Vault).
- Replaced stdout based `DEVENV_EXPORT:` protocol in tasks with file based exports (`$DEVENV_TASK_EXPORTS_FILE`), simplifying the encoding and moving JSON construction into Rust.
- Task exports are now always produced, including when a task is skipped via its `status` command.
- Validation errors for `@ready` process dependencies now include a link to the documentation.

### Breaking Changes

- Removed `devenv-tasks export` subcommand, replaced by file-based exports via `$DEVENV_TASK_EXPORTS_FILE`.

## 2.0.3 (2026-03-06)

### Bug Fixes

- Fixed `devenv test` conflicting with older pinned devenv modules that set `devenv.state` without `lib.mkDefault`.
- Removed debug watchexec logs from showing up in `--no-reload` shells.
- Fixed TUI colors being unreadable on light terminal backgrounds.
- Disabled the PTY in non-interactive environments (CI) preventing the shell from hanging.
- Fixed `navigateExclusion ranges overlap` error in the Nix backend ([#2552](https://github.com/cachix/devenv/issues/2552)).
- Increased thread stack size to 64MB to prevent stack overflows during deep Nix evaluations ([#2555](https://github.com/cachix/devenv/issues/2555)).

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
