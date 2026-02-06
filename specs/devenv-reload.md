# devenv-reload Specification

This document describes the architecture and design of the devenv-reload system, which provides hot-reload functionality for devenv shell sessions.

## Problem Statement

Traditional shell reload mechanisms require killing and respawning the shell when configuration files change, disrupting the user's interactive session (losing terminal state, shell history, and running processes).

## Solution

A PROMPT_COMMAND-based hot-reload system that writes updated environments to a temporary file and sources them automatically on the next shell prompt, providing seamless updates without interruption.

## Architecture Overview

The reload system uses an **inverted architecture** with three main components:

```
devenv (main CLI)
  ├── run_reload_shell() [main.rs]
  │   ├── Pre-computes shell environment (while TUI active)
  │   ├── Spawns ShellCoordinator (background task)
  │   ├── Spawns PtyExecutor thread (for task execution)
  │   └── Runs ShellSession (owns PTY and terminal)
  │
  ├── ShellCoordinator [devenv-reload/src/coordinator.rs]
  │   ├── Watches files via FileWatcher
  │   ├── Coordinates builds via DevenvShellBuilder
  │   └── Sends commands to ShellSession
  │
  └── ShellSession [devenv-shell/src/session.rs]
      ├── Owns PTY and terminal
      ├── Receives shell commands from coordinator
      ├── Executes tasks inside PTY via PtyTaskRunner
      └── Manages terminal I/O and status line
```

## Core Components

### 1. DevenvShellBuilder (`devenv/src/reload.rs`)

Implements the `ShellBuilder` trait to create shell commands on each build.

**Key Responsibilities:**
- Writes environment script to `$DEVENV_STATE/shell-env.sh`
- Creates rcfile with PROMPT_COMMAND hook setup
- Pre-computes environment while TUI active (avoids deadlocks with activity macro)
- Queries eval cache for watch files

**Two Build Methods:**

1. **`build()` - Initial Shell Spawn**
   - Called once at startup via coordinator
   - Returns interactive bash with PROMPT_COMMAND hook
   - Sets up `__devenv_reload_hook()` function

2. **`build_reload_env()` - Hot-Reload Build**
   - Called on file changes
   - Builds environment via `devenv.print_dev_env(false)`
   - Writes atomically to temp file, then renames (crash-safe)
   - Queries eval cache for new file inputs

**PROMPT_COMMAND Hook:**

```bash
__devenv_reload_apply() {
    # Source new environment if a reload is pending
    if [ -f "$DEVENV_RELOAD_FILE" ]; then
        source "$DEVENV_RELOAD_FILE"
        rm -f "$DEVENV_RELOAD_FILE"
        # Update saved PATH with new devenv environment
        _DEVENV_PATH="$PATH"
    fi
}

__devenv_restore_path() {
    # Restore devenv PATH (in case direnv or other tools modified it)
    export PATH="$_DEVENV_PATH"
}

__devenv_reload_hook() {
    __devenv_restore_path
}

# Keybinding: manual reload via Alt-Ctrl-R
if [[ $- == *i* ]] && command -v bind >/dev/null 2>&1; then
    __devenv_reload_keybind="${DEVENV_RELOAD_KEYBIND:-\e\C-r}"
    bind -x "\"${__devenv_reload_keybind}\":__devenv_reload_apply"
fi

# Append hook so it runs AFTER direnv's _direnv_hook
PROMPT_COMMAND="${PROMPT_COMMAND:+$PROMPT_COMMAND;}__devenv_reload_hook"
```

**Key Design Details:**
- `_DEVENV_PATH` saved before `.bashrc` (direnv might modify PATH)
- `_DEVENV_PATH` restored after `.bashrc` to preserve devenv's PATH
- Manual reload via **Alt-Ctrl-R** keybinding (configurable via `DEVENV_RELOAD_KEYBIND`)
- Automatic reload on next prompt after coordinator writes new env

### 2. ShellCoordinator (`devenv-reload/src/coordinator.rs`)

Orchestrates file watching and build coordination as a background task.

**Does NOT own PTY** - runs as background task coordinating with ShellSession.

**Event Loop:**

1. **File Change Event**
   - Detects file content changes via blake3 hash
   - Skips redundant changes (same hash)
   - Cancels any running build (debouncing)
   - Sends `Building { changed_files }` to ShellSession
   - Spawns `build_reload_env()` in background

2. **Reload Build Complete**
   - Sends `ReloadReady { changed_files }` or `BuildFailed { error }`
   - Shell's PROMPT_COMMAND hook picks it up on next prompt

3. **TUI Events**
   - `Exited`: Shell exited, coordinator shuts down
   - `Resize`: Handled by ShellSession directly

**Communication Channels:**
- Sends: `ShellCommand` → ShellSession (Spawn, Building, ReloadReady, BuildFailed, Shutdown)
- Receives: `ShellEvent` from ShellSession (Exited, Resize)

### 3. ShellSession (`devenv-shell/src/session.rs`)

Owns PTY and terminal, manages shell I/O and task execution.

**Lifecycle:**

```
run() entrypoint
  ├── Receives initial Spawn command with watch files
  ├── Spawns PTY early (tasks can run before TUI exits)
  ├── run_tasks_in_pty() - executes tasks before terminal handoff
  │   ├── Waits for __DEVENV_SHELL_READY__ marker
  │   ├── Receives PtyTaskRequest from coordinator
  │   ├── Injects commands with echo markers
  │   └── Captures output between markers
  ├── Signals TUI completion (backend_done_tx)
  ├── Waits for TUI to release terminal (terminal_ready_rx)
  ├── Enters raw mode
  ├── Runs event_loop() - main shell interaction
  │   ├── Forwards stdin to PTY
  │   ├── Reads PTY output, writes to stdout
  │   ├── Handles ShellCommand events from coordinator
  │   └── Draws status line
  └── Kills PTY on exit
```

**Task Execution in PTY:**

Uses marker-based protocol to inject and capture commands (via `PtyTaskRunner`):

```bash
# Injected command format:
echo '__DEVENV_TASK_START_<id>__'
export VAR='value'
cd /path
<command>
echo '__DEVENV_TASK_END_<id>_'$?'__'
```

- Waits for `__DEVENV_SHELL_READY__` marker (rcfile signals shell ready)
- For each task request: injects marked commands via PTY stdin
- Strips ANSI codes and trims whitespace for robust marker detection
- Captures stdout lines between start/end markers
- Extracts exit code from end marker

**Status Line:**

Updates bottom-of-terminal status (via `StatusLine`):
- Building: Blue background, shows changed files
- Reloading: Yellow background
- Ready: Green if successful, Red if failed
- Shows keybinding hint: "press Alt-Ctrl-R" (or configured binding)

## Crate Structure

### devenv-shell (Library)

Standalone crate for shell/PTY management with no devenv-specific dependencies.

| Module | Purpose | Key Types |
|--------|---------|-----------|
| `lib.rs` | Public API exports | - |
| `protocol.rs` | Communication types | `ShellCommand`, `ShellEvent`, `PtyTaskRequest`, `PtyTaskResult` |
| `pty.rs` | PTY wrapper | `Pty`, `PtyError`, `get_terminal_size()` |
| `terminal.rs` | Terminal utilities | `RawModeGuard`, `is_tty()` |
| `session.rs` | Main orchestrator | `ShellSession`, `SessionConfig`, `SessionError`, `TuiHandoff` |
| `status_line.rs` | Status bar rendering | `StatusLine`, `StatusState`, `StatusRenderer`, `DefaultStatusRenderer` |
| `task_runner.rs` | PTY task execution | `PtyTaskRunner`, `TaskRunnerError`, `strip_ansi_codes()` |

### devenv-reload (Library)

Coordinates file watching and build triggers. Re-exports types from devenv-shell for backwards compatibility.

| Module | Purpose | Key Types |
|--------|---------|-----------|
| `lib.rs` | Public API exports | Re-exports from devenv-shell |
| `coordinator.rs` | Build coordination logic | `ShellCoordinator`, `CoordinatorError` |
| `builder.rs` | Shell builder trait | `ShellBuilder`, `BuildContext`, `BuildTrigger`, `BuildError` |
| `manager.rs` | Legacy standalone manager | `ShellManager`, `ManagerMessage` (for non-TUI mode) |
| `watcher.rs` | File watching | `FileWatcher`, `WatcherHandle`, `FileChangeEvent` |
| `config.rs` | Configuration | `Config` with watch_files and reload_file |

### devenv/src/reload.rs

Implementation of `ShellBuilder` trait for devenv:

```rust
pub struct DevenvShellBuilder {
    handle: Handle,
    devenv: Arc<Mutex<Devenv>>,
    cmd: Option<String>,
    args: Vec<String>,
    initial_env_script: String,
    bash_path: String,
    dotfile: PathBuf,
    eval_cache_pool: Option<SqlitePool>,
    shell_cache_key: Option<EvalCacheKey>,
}
```

- Wraps `Devenv` in `Arc<Mutex<>>` for thread-safe access in background builds
- Stores pre-computed environment to avoid deadlocks with activity macro
- Queries eval cache for watch file dependencies

### devenv-tasks/src/executor.rs

Task Executor Abstraction:

```rust
pub trait TaskExecutor: Send + Sync {
    async fn execute(
        &self,
        ctx: ExecutionContext,
        callback: &dyn OutputCallback,
        cancellation: CancellationToken,
    ) -> ExecutionResult;
}
```

**Two Implementations:**

1. **SubprocessExecutor** (default)
   - Spawns tasks as separate processes
   - Used in command mode and `--no-reload`

2. **PtyExecutor** (for hot-reload)
   - Sends task requests via channel to ShellSession
   - Executes inside interactive shell via PTY
   - Used in interactive mode when reload enabled

## Data Flow: File Change to Shell Reload

```
File Change Detection
    ↓
FileWatcher (notify crate)
    ↓
ShellCoordinator::Event::FileChange
    ├─ Hash file to detect actual change
    ├─ Send ShellCommand::Building
    └─ Spawn build_reload_env() in background
         ↓
    DevenvShellBuilder::build_reload_env()
         ├─ Create new tokio runtime
         ├─ devenv.print_dev_env() - build new environment
         ├─ Atomically write to $DEVENV_STATE/pending-env.sh
         ├─ Query eval cache for new watch files
         └─ Send Event::ReloadBuildComplete
              ↓
    ShellCoordinator sends ShellCommand::ReloadReady
         ↓
    ShellSession receives ReloadReady
         ├─ Updates status line with ready message
         └─ Waits for user to press Alt-Ctrl-R
              ↓
    User presses Alt-Ctrl-R (or next prompt)
         ↓
    Bash PROMPT_COMMAND hook triggers
         ├─ __devenv_restore_path() - restore devenv PATH
         ├─ __devenv_reload_apply() - if pending-env.sh exists
         │   ├─ source pending-env.sh
         │   ├─ rm pending-env.sh
         │   └─ Update _DEVENV_PATH
         └─ Shell has new environment
```

## Configuration

### CLI Options

| Flag | Purpose | Default |
|------|---------|---------|
| `devenv shell` | Interactive shell with hot-reload | Enabled |
| `devenv shell --no-reload` | Shell without hot-reload | Disabled |
| `devenv shell "cmd"` | Run command with hot-reload setup | Enabled |
| `devenv shell "cmd" -- args` | Command with arguments | - |

### Environment Variables

| Variable | Purpose | Default |
|----------|---------|---------|
| `DEVENV_RELOAD_FILE` | Path to pending environment file | `/tmp/devenv-reload-<pid>.sh` |
| `DEVENV_RELOAD_KEYBIND` | Keybinding for manual reload | `\e\C-r` (Alt-Ctrl-R) |
| `DEVENV_STATE` | State directory for dotfiles | `$PWD/.devenv` |
| `DEVENV_TASK_OUTPUT_FILE` | Task output capture file | Set by task executor |

### Bash Argument Order

From `devenv/src/util.rs`:

```rust
pub const BASH_INTERACTIVE_ARGS_PREFIX: &[&str] = &["--noprofile", "--rcfile"];
pub const BASH_INTERACTIVE_ARGS_SUFFIX: &[&str] = &["-i"];
```

Usage: `bash --noprofile --rcfile <path> -i`

**Note:** `-i` must come AFTER `--rcfile` because bash parses long options first, then short options.

## Design Patterns & Rationales

### Pre-Computed Environment

**Problem:** `get_dev_environment()` has `#[activity]` macro which needs TUI, but TUI waits for build to complete → deadlock.

**Solution:** Compute environment in `run_reload_shell()` while TUI active, pass to builder.

### Separate Runtime for Reload Builds

**Problem:** Reload builds called from `spawn_blocking` context. If main runtime shutting down (shell exited), calling blocking operations causes panic.

**Solution:** Create dedicated single-threaded runtime for each reload build.

### PROMPT_COMMAND-based Reload

**Problem:** Killing/respawning shell disrupts session state and history.

**Solution:**
- Coordinator writes new env to temp file
- Shell hook sources it automatically on next prompt
- User can trigger manually via Alt-Ctrl-R keybinding

### Marker-based PTY Task Execution

**Problem:** Need to inject and capture commands inside PTY for task execution in shell context.

**Protocol:**
- Unique ID per task prevents command echo from interfering
- Echo markers delimit output
- Strip ANSI codes for robust marker detection
- Capture exit code in end marker

### Interactive vs Command Mode

| Mode | Tasks Run | Executor | Notes |
|------|-----------|----------|-------|
| Interactive (default) | Inside PTY | PtyExecutor | Runs after shell ready, before terminal handoff |
| Command mode | Before PTY | SubprocessExecutor | PTY immediately execs command and exits |
| `--no-reload` | Before shell | SubprocessExecutor | No hot-reload capability |

## Terminal Handoff Sequence

```
TUI Backend
    ├─ Spawns ShellSession task
    ├─ run_reload_shell() pre-computes env
    ├─ TUI shows activity display
    │
ShellSession
    ├─ Receives initial Spawn command
    ├─ Spawns PTY
    ├─ run_tasks_in_pty() (tasks still in TUI context)
    │
TUI Backend completes
    ├─ Sends backend_done_tx
    ├─ TUI exits, returns terminal control
    │
ShellSession
    ├─ Receives terminal_ready_rx
    ├─ Enters raw mode
    ├─ Enters event loop
    └─ Takes over terminal I/O
```

## Watch File Management

### Initial Watch Files

Queries eval cache after each build via `get_file_inputs_by_key_hash()`.

Watches all files that were inputs to shell evaluation:
- `devenv.nix`, `devenv.yaml`, `flake.nix`
- Imported files (`include_file`, `import`)
- Local Nix files (not `/nix/store` - immutable)

### Dynamic Watch Path Addition

`WatcherHandle::watch()` called during builds allows adding new watch paths at runtime as new dependencies are discovered.

## Error Handling

### Build Failures

If build fails (Nix evaluation error):
- Coordinator sends `ShellCommand::BuildFailed { error }`
- Status line shows error in red
- Shell keeps previous environment
- User can fix config and try again

### PTY Failures

If PTY task execution fails:
- Result includes error message
- Task execution fails but shell stays alive
- User can retry manually

### File Watcher Errors

If file watch setup fails:
- Error logged via tracing
- Coordinator continues with existing watch list
- May miss some file changes

## Key Files Reference

| Path | Purpose |
|------|---------|
| `devenv/src/reload.rs` | DevenvShellBuilder implementation |
| `devenv/src/main.rs` | run_reload_shell() entry point |
| `devenv/src/cli.rs` | CLI argument definitions |
| `devenv/src/util.rs` | Bash argument constants |
| `devenv-shell/src/session.rs` | ShellSession |
| `devenv-shell/src/pty.rs` | PTY wrapper |
| `devenv-shell/src/status_line.rs` | Status line rendering |
| `devenv-shell/src/task_runner.rs` | PtyTaskRunner |
| `devenv-shell/src/protocol.rs` | ShellCommand, ShellEvent, PtyTaskRequest |
| `devenv-reload/src/coordinator.rs` | ShellCoordinator |
| `devenv-reload/src/manager.rs` | ShellManager (legacy) |
| `devenv-reload/src/builder.rs` | ShellBuilder trait |
| `devenv-reload/src/watcher.rs` | FileWatcher |
| `devenv-tasks/src/executor.rs` | TaskExecutor trait |

## Component Interaction Diagram

```
┌─────────────────────────────────────────────────────────────┐
│                      devenv CLI                             │
│  Commands::Shell { no_reload }                              │
└──────────────────┬──────────────────────────────────────────┘
                   │
         ┌─────────┴──────────┐
         │                    │
    no_reload=false      no_reload=true
         │                    │
         ▼                    ▼
  run_reload_shell()    shell() / prepare_exec()
         │                    │
         ├─ Pre-compute       └─ Direct execution
         │  environment
         │
         ├─ Create
         │  DevenvShellBuilder
         │
         ├─ Spawn tasks
         │  (mode dependent)
         │
    ┌────┴──────────────────────────┐
    │                               │
    ▼                               ▼
ShellCoordinator              ShellSession
[devenv-reload]               [devenv-shell]
(file watcher,                (PTY I/O,
 build coord)                  task exec)
    │                               │
    ├─ FileWatcher                  ├─ PTY spawn
    │   notify                       │
    │                               ├─ raw mode
    ├─ DevenvShellBuilder::build()   │
    │   initial                      ├─ StatusLine
    │                               │
    ├─ File change                  ├─ Event loop
    │   detected                    │
    │                               ├─ stdin/PTY I/O
    ├─ DevenvShellBuilder::          │
    │   build_reload_env()           └─ Terminal
    │                                  management
    │
    └─ ShellCommand channels ────────►
       (Spawn, Building,
        ReloadReady, Failed,
        Shutdown)
```

## Dependency Graph

```
                    devenv (main)
                    /     |     \
                   /      |      \
            devenv-tui  devenv-reload  devenv-tasks
                 |        |       \       /
                 |        |        \     /
                 |        |      devenv-shell
                 |        |           |
                 v        v           v
            iocraft    notify    portable-pty
```

**devenv-shell** (no devenv-* dependencies):
- portable-pty, crossterm, avt, tokio, regex, libc (unix)

**devenv-reload**:
- devenv-shell (protocol types, re-exports)
- notify, blake3

**devenv-tui**:
- iocraft, devenv-activity
- (no devenv-shell dependency - just UI)

**devenv-tasks**:
- devenv-shell (for PtyTaskRequest, PtyTaskResult)
- tokio

**devenv** (main):
- devenv-shell, devenv-reload, devenv-tui, devenv-tasks

## Summary: Traditional vs. devenv-reload

| Aspect | Traditional | devenv-reload |
|--------|-------------|---------------|
| **Approach** | Kill PTY, spawn new one | Write env to file, source via PROMPT_COMMAND |
| **User Impact** | Session interrupted | Seamless update on next prompt |
| **Terminal State** | Lost | Preserved |
| **Shell History** | Reset | Maintained |
| **Configuration** | N/A | Enabled by default, disable with `--no-reload` |
| **Manual Trigger** | None | Alt-Ctrl-R keybinding |
| **Task Execution** | Subprocess | PtyExecutor runs in shell context |
| **File Changes** | Manual restart | Dynamic watch via eval cache |
