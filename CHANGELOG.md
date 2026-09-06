# Changelog

## 2.3.0 (unreleased)

### Bug Fixes

- Fixed the interactive `devenv shell` session crashing with "terminal error: invalid value" when the terminal briefly reports a `0x0` size, most commonly seen on WSL2 right after `devenv`'s shell hook auto-activates on `cd`.
- Certificate generation uses Nix-provided mkcert and its helper tools instead of relying on the development shell's `PATH`.
- Fixed `devenv up` incorrectly reporting that the proxy exited while its capability broker child was still dropping root privileges.
- Fixed environment capture accumulating unbounded `.devenv/shell-*.sh` files. Capture-specific activation scripts are now temporary, and non-interactive commands are passed as arguments instead of being embedded in persistent activation scripts ([#3149](https://github.com/cachix/devenv/issues/3149)).
- Restored dotenv compatibility with older devenv CLIs whose project inputs resolve newer modules. These CLIs now fall back to the legacy Nix parser instead of failing because the `loadDotenv` primop is unavailable.
- Fixed `devenv shell` hanging on exit or repeatedly reloading when a configured dotenv file was missing, especially in large repositories.
- Fixed `devenv tasks run` leaving processes running after it exits, in both TUI and non-TUI mode. A task depending on a process, such as `after = [ "devenv:processes:postgres@ready" ]`, started that process but never stopped it, so services like PostgreSQL and Redis were orphaned and kept holding their ports and data directories.
- Interrupting devenv a second time no longer leaves processes running. The second Ctrl+C exits straight away instead of waiting for shutdown to finish, which used to abandon any process still shutting down. This was easy to hit, since a process that is slow to stop is given five seconds before it is killed.
- Existing GC root symlinks are now updated atomically when their store path changes.
- Fixed JSON trace output (`--trace-to json:...`) writing invalid JSON lines for activity events that contain lists, such as the task hierarchy event.

### Improvements

- Added opt-in HTTPS process URLs with `processes.<name>.proxy.https.enable`, using the project's existing mkcert certificate authority.
- Processes with declared ports get `.localhost` HTTP URLs by default, with hostname overrides and URLs shown in the TUI ([#3141](https://github.com/cachix/devenv/pull/3141)).
- `devenv hook <shell>` can now pass arguments to its auto-activated `devenv shell`. Arguments following `--` are forwarded safely in Bash, Zsh, Fish, and Nushell, for example `devenv hook fish -- --no-tui` ([#3128](https://github.com/cachix/devenv/issues/3128)).
- OTLP trace destinations now also export Nix evaluator heap and garbage-collection metrics for diagnosing memory use.
- When the Nix daemon is running version 2.35 or newer, `devenv gc` now cleans up old environments in a single batch, making it much faster. It also shows clear progress while it runs.
- File watching now uses substantially fewer allocations and less peak memory for large dependency sets, batches registrations and change bursts more efficiently, and keeps tracking files that are created later or replaced atomically.
- Pinning several package versions no longer costs a separate nixpkgs fetch and evaluation for each one. `multiverse.pins { cmake = "3.26.4"; bun = "0.7.0"; }` resolves a whole set of versions, at exactly those versions, through the fewest nixpkgs revisions that can serve them and returns the packages.
- Replaced the Nix-based dotenv parser with `dotenv-ng`, called through the devenv CLI during evaluation. Dotenv files now support quotes, multiline values, comments, `export`, optional variable substitution, ordered files in subdirectories, task-generated files, and hot reloads. Values remain available through `config.env`, explicit `env` definitions take precedence, and file and substitution dependencies invalidate the evaluation cache when they change ([#775](https://github.com/cachix/devenv/issues/775), [#1208](https://github.com/cachix/devenv/issues/1208), [#1333](https://github.com/cachix/devenv/issues/1333), [#1591](https://github.com/cachix/devenv/issues/1591), [#1652](https://github.com/cachix/devenv/issues/1652), [#1994](https://github.com/cachix/devenv/issues/1994), [#2176](https://github.com/cachix/devenv/issues/2176)).
- `devenv-run-tests` reports the runtime and shell closure size of every test and can fail a test whose closure exceeds `max_closure_size` in its `.test-config.yml`.
- Reduced the size of the devenv closure from 528 MB to 376 MB. It no longer contains duplicate copies of glibc, OpenSSL, curl, Boost, ICU and the Nix libraries, which were pulled in by `cachix` and `nixd` being built against different package sets than devenv itself.
- `devenv tasks list` now prints a more readable human tree: task names are colored (processes in cyan), descriptions are shown inline, and details like status checks and watched files are broken onto their own indented lines instead of being crammed into a single parenthetical.
- Module errors now point at the `devenv.nix` (or imported module) that defined the offending option, instead of `<unknown-file>`.
- Updated the bundled Nix from `v2.34.8` to `v2.35.2`.
- Added `processes.<name>.shutdown.signal` and `.grace` for the native manager and process-compose. The same settings apply to restarts, and PostgreSQL now uses SIGINT for fast shutdown.
- Added process-manager capability checks. Detached mode is supported by process-compose, Honcho, Hivemind, and Overmind; unsupported operations now fail before launch, and mprocs remains foreground-only.
- Process-manager metadata is compatible with older CLIs and Nix modules; no public Nix options changed.
- Enabling `--trace-to` no longer serializes every activity event up front. Trace sinks now walk the typed event only when they write it, so tracing no longer allocates a JSON tree per Nix build log line on the Nix logger thread. Benchmarks of JSON activity export show 2.6× the throughput, 89% fewer allocation calls, and 78% fewer allocated bytes. With trace output disabled, lazy config logging eliminates serialization entirely—3,967 allocations and 287 KB for a representative 128-task config.
- Process activities in `--trace-to json` output now carry structured `ports` and `ready_probe` fields, and process exits and supervisor restarts are exported as `exited` and `restarted` events instead of free-form log lines. The TUI and console show a `Process exited (success)` or `Process exited (failure)` line for every exit.

### Bug Fixes

- Fixed short-lived processes staying active under process-compose. External managers now own restart and readiness, while `devenv-tasks` runs the process once and exits when it settles ([#2879](https://github.com/cachix/devenv/issues/2879)).
- Fixed services surviving a crash of `devenv-tasks` or the native process manager. A guardian now cleans abandoned service sessions, and the next manager reconciles them before starting the same process.
- Fixed `devenv-tasks` leaving processes running after errors or parent death. It now stops processes before returning and when it loses its parent.
- Fixed the native manager missing descendants in separate process groups. Stop and restart now clean the whole service session.
- Fixed `devenv down` leaving external-manager descendants running. Shutdown now tracks the complete process scope and gracefully stops Overmind before cleanup.
- Fixed external-manager start/stop races, false-positive detached starts, and process names containing shell metacharacters.
- Fixed `devenv processes down` returning before Overmind had finished stopping. Overmind left its control socket behind, and the next `devenv up` refused to start. devenv now waits for the manager to exit before it cleans up.
- Fixed a foreground `devenv up` with an external process manager being invisible to other devenv commands. `devenv processes down` in another terminal now stops it, and a second `devenv up` attaches instead of starting a rival manager.
- Fixed `devenv processes down` doing nothing after you log out and back in. systemd removes `/run/user/$UID` when your last session ends, which used to lose a detached process manager: it kept running, nothing could stop it, and the next `devenv up` started a second one beside it. devenv now keeps a copy of the manager state in `.devenv`.
- Fixed devenv suppressing the TUI and forcing quiet output for every shell started from Warp. AI-agent auto-detection now matches only autonomous agents, not "hybrid" environments.

## 2.2.2 (2026-08-13)

### Bug Fixes

- Fixed `devenv mcp` ignoring termination signals. A single SIGTERM or SIGINT now shuts the server down immediately, without waiting for further input on stdin; previously it kept running until it was killed forcefully or its client closed stdin ([#3065](https://github.com/cachix/devenv/issues/3065)).
- Fixed `devenv mcp` ignoring most of the session's configuration; it previously fell back to default Nix, cache, and shell settings for everything except inputs and imports.
- Fixed devenv exporting an inferred SecretSpec provider into the development shell when no provider override was configured, which replaced per-secret provider fallback chains for commands run inside the shell.

## 2.2.1 (2026-08-02)

### Bug Fixes

- Fixed profiles failing to override package-valued options, such as `languages.python.package`, unless the profile
  used `lib.mkForce`. Automatic profile priorities now apply to derivation values while mergeable package lists
  continue to combine ([#3057](https://github.com/cachix/devenv/issues/3057)).
- Fixed `devenv hook zsh` and the other shell hooks starting Bash when an editor terminal inherited a valid but stale Bash `$SHELL`. Auto-activation now passes the known outer shell as a non-authoritative hint, below explicit CLI, environment, and `devenv.yaml` shell settings but above ambient `$SHELL` detection ([#2880](https://github.com/cachix/devenv/issues/2880)).
- Fixed `devenv shell` occasionally losing the exit code of the shell it ran, reporting success for a session that exited nonzero.
- Fixed `devenv --profile <name> allow` ignoring the selected profile for projects with a local `devenv.nix`. Allowed in-tree projects now persist their auto-activation profiles just like out-of-tree `--from` bindings; explicit `--profile` flags still take priority.
- Fixed `devenv init` hanging when an existing file differs from the template and there is no terminal to confirm on.
- Fixed devenv freezing until killed when run on a terminal from a background process group, for example under GNU `timeout` or other wrappers that start commands in their own process group.
- Fixed `devenv shell` failing with an incorrect `out` environment variable when the experimental `ca-derivations` feature is enabled ([#2364](https://github.com/cachix/devenv/issues/2364)).

## 2.2.0 (2026-07-28)

### Bug Fixes

- Fixed TUI commands continuing to run as CPU-spinning background processes after their terminal closed.
- Fixed a potential stack overflow in `devenv mcp` while it indexed packages and options in the background. Indexing now runs with the large stack required by Nix evaluation and is interrupted when the server exits instead of delaying shutdown.
- Fixed several failures in Cachix netrc setup: authenticated pulls could return HTTP 401, devenv could replace credentials in Nix's existing `netrc-file`, and caches could disappear as substituters when the managed netrc could not be prepared. devenv now installs credentials before opening the Nix store, merges them with Nix's existing `netrc-file` instead of replacing it, and keeps the cache available when a netrc file cannot be read or written. Generated credential files are private to the user and unique per process; stale files left behind by crashed or killed processes are removed.
- Fixed SecretSpec providers such as SOPS being unavailable from the bundled CLI by enabling SecretSpec's default provider features.
- Fixed the SecretSpec profile and provider selected in `devenv.yaml` being applied only during `devenv.nix` evaluation. devenv now exports the resolved values as `SECRETSPEC_PROFILE` and `SECRETSPEC_PROVIDER` in the development shell, so `secretspec run` and other SecretSpec commands use the same configuration. The matching `secretspec` CLI is also included in the devenv package instead of requiring a separate installation.
- Fixed the Nushell hook failing to deactivate the environment when `cd`-ing out of a project. Nushell's `exit` cannot terminate a shell from an environment-change hook: it reported `Exit doesn't catch internally` and left the hook-spawned shell running. The hook now terminates that shell so the parent hook can resume and follow `.devenv/exit-dir` ([#3033](https://github.com/cachix/devenv/issues/3033)).
- Fixed shell hooks skipping reactivation when returning immediately to a project after `cd`-ing out. The activation guard remained set after the parent hook followed the user out, so the environment only reactivated after leaving and returning a second time. The guard is now cleared for Bash, Zsh, Fish, and Nushell ([#3034](https://github.com/cachix/devenv/issues/3034)).
- Fixed `devenv tasks run` hanging indefinitely when a task depended on a process with a `ready` probe (for example a task gated on `services.mysql`). The process started, but its readiness probe could not run, so the process never became ready and the dependent task waited indefinitely. The same configuration already worked under `devenv up` and `devenv test` ([#3030](https://github.com/cachix/devenv/issues/3030)).
- Fixed a process crashing with "address already in use" when a second project reused a port still held by the first. After a project's detached process manager was stopped (`devenv up -d` then `devenv processes down`), its control socket could remain responsive briefly after its PID file was removed. A subsequent `devenv up` would then reuse stale port allocations instead of choosing a free port. devenv now reads running-manager port allocations only from a manager backed by a live PID file.
- Fixed `imports` in `devenv.yaml` silently dropping a directory imported transitively through another project's `devenv.yaml`, rather than directly by the root project, when that directory had no `devenv.yaml` of its own. Its `devenv.nix` is now loaded regardless of the import depth.
- Fixed editor-launched terminals and stripped environments defaulting to Bash when `$SHELL` is absent. devenv now checks the user's login shell from the system account database before defaulting to Bash, including under `sudo` and `su`. The shell's absolute path is used in both PTY and non-PTY interactive sessions, so it works even when `$PATH` does not contain its Nix store directory; stale login-shell paths fall back to the standard shell-resolution logic. devenv now also warns when an unsupported shell is explicitly requested via `--shell` or `devenv.yaml` before falling back to Bash ([#2880](https://github.com/cachix/devenv/issues/2880), [#2992](https://github.com/cachix/devenv/pull/2992)).
- Fixed the shell hook closing a pane or session in a terminal multiplexer such as tmux or zellij when leaving a project directory. Shells started from an active devenv shell, such as a new pane, an SSH session, or a manually started nested shell, inherited the hook marker and incorrectly treated themselves as hook-spawned. The marker is now consumed and removed from the environment as soon as the intended shell receives it, so it cannot leak to processes started afterward ([#2861](https://github.com/cachix/devenv/issues/2861)).
- Fixed a hook-activated shell sometimes remaining active after leaving a project outside a terminal multiplexer. Deactivation depended on the spawned shell's rc file re-sourcing the hook script, which some configurations skip (for example, a Fish config gated behind `status is-login`, which a hook-spawned non-login `fish -i` never satisfies). Each shell's generated init file now handles leaving the project directly, independently of the user's rc file.
- Fixed the Fish hook spawning a redundant devenv shell over an environment that direnv or another tool had already activated for the same directory via `use devenv` in `.envrc`. The hook decides whether to activate at one prompt and acts at the next to avoid spawning during `cd`, but it did not recheck whether another tool had activated the directory in the meantime.
- Fixed `cd -` not returning to the project directory after the Fish hook followed the user out of it ([#2853](https://github.com/cachix/devenv/issues/2853)). The hook uses `builtin cd` to avoid retriggering a user-overridden `cd` (for example, `zoxide init --cmd=cd`), but this bypasses Fish's directory-history bookkeeping. The hook now updates that history itself.
- Fixed input-relative imports (for example, `imports: [myinput/some/dir]`) being wrongly rejected with "Import path resolves outside the git repository" when a symlinked path was used to reach the project root, as occurs with macOS temporary directories under `/var`.
- Fixed `devenv shell` appearing to hang in the TUI when the file watcher failed to register a watch, most commonly after reaching the inotify watch limit (`fs.inotify.max_user_watches`). The watcher now continues after a failed registration and surfaces a warning in the TUI instead of logging it only under `--verbose --no-tui`.
- Fixed a task whose `status` command exits nonzero being rendered as a failed `check status` activity, making successful runs look like they contained a failure. A nonzero status is a normal cache miss that runs the task's command; only a failure to spawn the status command itself is now reported as a failure ([#2984](https://github.com/cachix/devenv/issues/2984)).
- Fixed `devenv processes restart` leaving a restarted process reported as "exited" by `devenv processes list`/`status` even though it was running. When the process had previously exited (restart policy said stop), the restart spawned a fresh job and supervisor but never published a new status, and processes without readiness probes produce no further status events until they exit again. Restarting such a process now also replaces a supervisor parked after exit (kept alive only for file watching), which previously stopped monitoring the restarted job ([#2982](https://github.com/cachix/devenv/issues/2982)).
- Fixed devenv overriding user-configured experimental features in `nix.conf` ([#2931](https://github.com/cachix/devenv/issues/2931)).
- Fixed `devenv hook bash`/`zsh` skipping activation for a sibling project after following `.devenv/exit-dir` from a hook-spawned shell that left the previous project ([#2944](https://github.com/cachix/devenv/issues/2944)).
- Fixed misleading evaluation errors where evaluation warnings replaced the real error message, hiding syntax errors and suggestions like `devenv inputs add …` ([#2820](https://github.com/cachix/devenv/issues/2820)).
- Fixed `devenv shell` failing with `Failed to parse output store path … is not in the Nix store` when using a relocated or chrooted Nix store (e.g. `NIX_REMOTE='local?root=…'` or a bind-mounted store). The realized shell path was translated to its physical location and then fed back into store-path parsing, which only accepts the logical `/nix/store/…` form; the logical path is now recovered before creating the GC root ([#2499](https://github.com/cachix/devenv/issues/2499)).
- Fixed the TUI crashing on activity names or store paths containing multibyte characters (e.g. non-ASCII package names or evaluation paths) when shortened for narrow terminals.
- Fixed the TUI crashing with an arithmetic overflow when a download or task reported progress beyond its expected total.
- Fixed the quit confirmation prompt overflowing past the right edge on terminals narrower than about 82 columns; the prompt now adapts to the available width.
- Fixed the bottom navigation hint bar overflowing the right edge on standard 80-column terminals when a process was selected; the hints are now more compact on narrower terminals.
- Fixed devenv invocations with different `$TMPDIR` values choosing different runtime directories and therefore failing to find each other's process manager. The runtime path now uses `$XDG_RUNTIME_DIR`, falling back to `/tmp`; `$TMPDIR` is no longer consulted. A daemon still using its previous runtime path may need to be restarted once ([#2923](https://github.com/cachix/devenv/issues/2923)).
- Fixed edits to local `path:` inputs (e.g. `url: path:../shared-config`) never taking effect: the evaluation cache served the old configuration until `.devenv` was deleted, and `devenv update` did not help. Files from local path inputs are now read directly from disk and tracked by the cache, so changes apply on the next command and direnv reloads when they change.
- Fixed the evaluation cache missing a `devenv.local.nix` (or any other tracked file) created within the same second as the previous evaluation, which could happen in scripts that run devenv commands back to back.
- Fixed edits to `enterShell` causing a parse error (e.g. `(eval):6: parse error near '\n'`) when a running `devenv shell` reloaded under Zsh or Fish. The `enterShell` output was being mixed into the environment data applied during reload; it now goes to the terminal as it does on a fresh shell entry ([#2919](https://github.com/cachix/devenv/issues/2919)).
- Fixed `devenv up` (and other TUI commands) burning significant CPU while idle — roughly 10-15% per managed process — even when every process was quiet. The foreground UI was recomputing its layout dozens of times per second regardless of whether anything changed, and each idle process woke the whole UI twice a second. The TUI now only redraws when the model actually changes, with a slow heartbeat to keep elapsed timers ticking ([#2915](https://github.com/cachix/devenv/issues/2915)).
- Fixed `devenv container` placing `copyToRoot` directories under a hash-prefixed subdirectory (e.g. `/env/<hash>-source`) instead of in the working directory. The project root and other directory paths now land directly under the working directory, and single files keep their original name ([#2914](https://github.com/cachix/devenv/issues/2914)).
- Fixed local files and directories pulled into the Nix store by path (e.g. `scripts.foo.exec = ./foo.sh;` or `languages.rust.import ./.`) not being tracked as eval-cache dependencies, so editing them returned stale results from `devenv build` and `devenv shell` until the cache was manually refreshed. Such sources are now tracked, with directories hashed recursively over their contents so nested edits are detected ([#2886](https://github.com/cachix/devenv/issues/2886), [#2893](https://github.com/cachix/devenv/issues/2893)).
- Fixed `devenv shell` lingering as a background process, often pinned at 100%+ CPU, after its terminal window or tab was closed. The shell now reacts to the SIGHUP/SIGINT/SIGTERM that already trigger devenv's graceful shutdown by killing the inner shell, instead of orphaning it ([#2845](https://github.com/cachix/devenv/issues/2845)).
- Fixed `devenv shell`/`devenv update` failing with `authentication required but no callback set` when a `url."ssh://git@github.com/".insteadOf` Git configuration rewrites GitHub HTTPS URLs to SSH. GitHub flake inputs now resolve over SSH using your SSH agent ([#2842](https://github.com/cachix/devenv/issues/2842)).
- Fixed the "N files" counter under "Evaluating shell" inflating from generic Nix log lines. The counter now only includes actual file reads.
- Fixed `devenv up`/`test`/`tasks` failing with `error: could not find a flake.nix file` when the devenv shell is loaded from a remote flake via direnv ([#2599](https://github.com/cachix/devenv/issues/2599)).
- Fixed `devenv shell` printing internal reload warnings (e.g. "Watched path became unavailable, forcing reload") to the user's terminal, clobbering scrollback, prompts, and editors.
- Fixed `devenv up` corrupting a running daemon's PID file and socket when started in the foreground, leaving the daemon unmanageable. Foreground `up` now rejects with "Processes already running" when a daemon is active.
- Fixed a remaining case in which the shell hook spawned a nested `devenv shell` after `devenv shell` was entered manually (a follow-up to [#2815](https://github.com/cachix/devenv/pull/2815)).
- Fixed "zoxide: infinite loop detected" when using `zoxide init --cmd=cd fish` and `cd`-ing into a devenv project. The Fish hook now defers spawning `devenv shell` to the next prompt instead of spawning inside the `PWD` event handler, so in-progress shell state never leaks into the devenv shell ([#2841](https://github.com/cachix/devenv/issues/2841)).
- Fixed the Nushell hook behaving differently from the Bash, Zsh, and Fish hooks: outer shells with `DEVENV_ROOT` exported (e.g. via direnv) no longer `exit` when leaving the project, and a manually entered `devenv shell` no longer respawns a nested shell.
- Fixed stale task and option results that lingered when `devenv.nix` was edited while a command was already evaluating. The eval cache no longer stores results whose tracked input files were modified mid-evaluation, so the next run sees the new definitions without deleting `.devenv/nix-eval-cache.db*` ([#2745](https://github.com/cachix/devenv/issues/2745)).
- Fixed long lines in `devenv shell` getting a hard newline inserted at the wrap point when copied to the clipboard. The shell now preserves soft wrapping when flushing output into the terminal's scrollback, so clipboard copies keep the original single line ([#2865](https://github.com/cachix/devenv/issues/2865)).
- Fixed files declared with the `files` option not being regenerated when an auto-loaded (`devenv allow`) shell reloaded after `devenv update`. `enterShell` tasks (including `devenv:files`) now rerun on hot reload, matching a fresh shell entry, instead of only updating environment variables ([#2864](https://github.com/cachix/devenv/issues/2864)).
- Fixed `devenv test --no-tui` (and any other non-TUI invocation) silently discarding all output from the `enterTest` script, so the test runner's output, traces, and failure messages never reached the terminal or CI logs. Output from commands run in the shell is now printed in non-TUI mode.
- Fixed `watch` paths being ignored for one-shot processes that exit immediately (e.g. code generators). The file watcher was torn down as soon as the process exited, so later edits never triggered a rerun. Watched one-shot processes now stay parked after exiting and rerun when a watched file changes.
- Fixed `devenv up` intermittently failing to start processes with `Failed to initialize task cache: … pool timed out while waiting for an open connection`, most often in CI. Processes launched by a non-native process manager (e.g. `process-compose`) opened the same task-cache database concurrently and raced to create and migrate it; the database is now initialized once before the processes start ([#2897](https://github.com/cachix/devenv/issues/2897)).
- Fixed `devenv inputs add` from a subdirectory writing to a stray `devenv.yaml` in the subdirectory instead of the enclosing project. It now walks up to find `devenv.nix` the same way `devenv shell` does, so the input is added where the rest of devenv reads it.
- Fixed `devenv gc` failing with "File devenv.nix does not exist" when run outside of a project. Garbage collection operates on the global devenv store and no longer requires a `devenv.nix` ([#2928](https://github.com/cachix/devenv/issues/2928)).
- Fixed unfree package errors suggesting only generic Nix/NixOS configuration. They now point devenv users to `allow_unfree: true` or `nixpkgs.permitted_unfree_packages` in `devenv.yaml` ([#2850](https://github.com/cachix/devenv/issues/2850)).
- Fixed `devenv shell` failing outright with a SQLite `disk I/O error` on filesystems that don't support shared-memory mmap (seen on virtiofs/9p VM mounts and similar). The eval cache and task cache databases now fall back to plain rollback-journal mode on these filesystems instead of crashing ([#2947](https://github.com/cachix/devenv/issues/2947)).
- Fixed failure diagnostics being buried in evaluation output in non-TUI mode. A failing evaluation no longer replays accumulated progress logs, such as `evaluating file …`, after the error, and `devenv-run-tests` now reports test failures after the diagnostic output instead of racing it.
- Fixed lifecycle races in the native process manager: a process being relaunched could be started twice, leaving an orphaned copy that kept its port bound but no longer appeared in `devenv processes list`; a process whose dependency could not be satisfied vanished from the list instead of showing as stopped; and a relaunched process that had previously failed left its dependents permanently blocked on the stale failure.
- Fixed `devenv processes wait` hanging forever when a process is waiting on a dependency that is stopped or not started.
- Fixed `devenv up -d` silently scheduling processes into another terminal's foreground `devenv up` session; it now asks you to attach with plain `devenv up` or stop the session first.
- Fixed `devenv up` attaching to a running process manager and then blocking, with no way to interrupt it, when run under an AI coding agent or with piped output. It now attaches only at an interactive terminal and otherwise reports that processes are already running.
- Fixed `devenv processes wait` returning before a process was up while it was still waiting for a one-shot setup task (e.g. a migration); a running setup task now counts as in progress.
- Fixed detaching from a running native process manager completing the attached process activities in the client TUI even though the daemon-owned processes were still alive. Attached rows are now non-owning proxies, and self-exited and crash-loop-exhausted processes retain distinct `exited` and `gave up` states.
- Fixed two concurrent cold `devenv up -d` invocations racing to spawn separate native managers for the same project, which could orphan the losing daemon and its children. Daemon startup is now serialized until one manager publishes its PID, after which the other invocation attaches normally.
- Fixed dynamically discovered process dependency closures remaining pending forever when an unseen one-shot failed or a shutdown cancelled it. Terminal failure and cancellation now propagate through the retained graph, and task cancellation reliably terminates the task's whole subprocess group.
- Fixed `processes.<name>.linux.capabilities` failing to grant the requested capabilities. A sudo-authenticated broker now grants only the declared capabilities, keeps services running as the invoking user, and supports detached processes and supervised restarts without repeated prompts. On macOS the option is ignored with a warning instead of silently, and a non-interactive `devenv up` without `sudo -v` only fails when a process that needs capabilities is actually being started.
- Fixed a shutdown racing with a task dependency failure reporting unstarted tasks as dependency failures instead of cancellations. Shutdown now consistently takes precedence across the remaining dependency closure.
- Fixed `devenv processes logs --lines` returning the wrong number of lines when the last log line had no trailing newline, and normalized CRLF output so stray carriage returns are not printed. Native manager port-allocation snapshots are now returned in a stable order as well.
- Fixed a process that exited on its own and was then explicitly stopped still showing as exited (and counting as succeeded in run summaries) instead of stopped.
- Fixed `devenv up` with no arguments not starting a process whose configuration omits `start.enable`, even though it defaults to enabled and `devenv up <name>` would start it.
- Fixed a process that exits on its own (a crash or a one-shot that runs to completion with no restart) continuing to show as `running` in the `devenv up` TUI after it had stopped; it now shows as exited, while an exhausted crash loop shows as failed.

### Improvements

- Added a `multiverse` package argument for selecting historical Nixpkgs packages by version, such as
  `multiverse.cmake."3.16.5"`, backed by an optional `nixpkgs-multiverse` input
  ([#16](https://github.com/cachix/devenv/issues/16)).
- `devenv shell` reload sessions now redraw only the terminal rows that changed instead of the whole screen every time output scrolls, reducing flicker and output bandwidth. Resizing the terminal mid-session no longer risks dropping pending lines from scrollback.
- Reduced renderer fragmentation in `devenv shell` by reading PTY output in larger batches, lowering syscall and event-allocation overhead during full-screen repaint bursts.
- `DEVENV_HOME` now overrides where devenv stores all per-user data (GC roots, trust database, cached keys), not just the trust database.
- Non-TUI console output is now buffered and flushed in batches, reducing write overhead during verbose evaluation.
- Traces now identify whether devenv was invoked by the CLI, direnv, or the native shell hook with a `devenv.caller` span attribute. Caller information is passed explicitly by integrations, so nested commands are not mistaken for automatic activation ([#2965](https://github.com/cachix/devenv/issues/2965)).
- Reduced the size of the devenv closure and container image by about 550 MiB by linking `nixd` (used by `devenv lsp`) statically against LLVM, so the monolithic LLVM shared library is no longer bundled.
- Reduced the size of the devenv binary's closure and container image by no longer bundling a debug build of `libghostty-vt`, which pulled Zig and LLVM (about 1 GiB) into every installation.
- Added `DEVENV_TRACE_DEFAULT_TO` for configuring default trace destinations that apply only when no explicit `--trace-to`, `DEVENV_TRACE_TO`, or legacy trace output is set. Set `DEVENV_TRACE_DEFAULT_TO` to an empty string to suppress the default for a command or session ([#2963](https://github.com/cachix/devenv/issues/2963)).
- Upgraded SecretSpec to 0.17.0, adding provider aliases, AWS Secrets Manager key prefixes, audit logging and access reasons, custom Bitwarden instances, and clearer provider-outage errors.
- Cachix now authenticates pulls and pushes without exporting `CACHIX_AUTH_TOKEN`: set `secretspec.cachix_auth_token` to `true` (or a custom secret name) to enable a built-in required SecretSpec secret without a `secretspec.toml` declaration. If SecretSpec does not resolve a token, devenv falls back to the auth token stored by the Cachix CLI (`cachix authtoken`) in `~/.config/cachix/cachix.dhall`. The resolved token is passed to the Cachix push daemon as well.
- Added `devenv tasks list --json` for machine-readable task graph inspection ([#2966](https://github.com/cachix/devenv/issues/2966)).
- `devenv repl` now exposes `inputs` alongside `devenv` and `pkgs`, so you can inspect inputs declared in `devenv.yaml` directly from the REPL (e.g. `inputs.nixpkgs.lib.version`).
- The TUI now shows each process's state as a status dot whose shape encodes the lifecycle (waiting, starting, running, ready, stopped, failed) instead of an identical spinner on every process. The state remains recognizable without relying on color, while transient states gently pulse to signal progress.
- Non-TUI console output now surfaces Nix evaluation and build progress while hiding internal debug noise.
- Automatic TUI disabling and quiet output now use the `detect-coding-agent` crate, expanding detection beyond Claude Code to Aider, autonomous and cloud-based agents, and others. Set `DEVENV_NO_AI_AGENT=1` to opt out.
- Added `devenv down` as a shorthand for `devenv processes down`, mirroring `devenv up` ([#2862](https://github.com/cachix/devenv/issues/2862)).
- `devenv hook fish` and `devenv hook nu` are now loaded automatically by Fish and Nushell via `share/fish/vendor_conf.d/devenv.fish` and `share/nushell/vendor/autoload/devenv.nu`, respectively. Bash and Zsh have no equivalent mechanism, so they still need manual configuration.
- Added a `--include-envrc` flag to `devenv init` (also settable via `DEVENV_INCLUDE_ENVRC`) to scaffold a direnv `.envrc` file ([#2859](https://github.com/cachix/devenv/pull/2859)).
- `devenv --from <source> allow` now binds a directory to an out-of-tree source, so you can use a devenv configuration without a local `devenv.nix`. Every subsequent `devenv` command in that directory loads its configuration from `<source>` without repeating `--from`, and the shell hook auto-activates the environment on `cd` just as it does for a local project.
- `--from path:<dir>` sources now load their full configuration: the source's `devenv.yaml` (inputs and imports, including sibling imports within its git repository) is merged, and its modules are imported from the live directory so edits apply immediately without re-fetching.
- `devenv --from <source> --profile <name> allow` also persists the selected profiles, so every subsequent command in the bound directory activates them automatically; explicit `--profile` flags still take priority.
- `devenv up` now attaches to an already-running process manager (started by `devenv up -d`) instead of failing with "Processes already running". It streams status, ports, and logs over the control socket; honors the requested process subset (e.g. `devenv up foo`) and `after`/`before` ordering; and reports when it has attached. On Ctrl-C, it prompts you to detach and leave the processes running or stop the whole manager ([#971](https://github.com/cachix/devenv/issues/971)).
- Added `devenv processes attach` to attach to running processes and stream their status, ports, and logs until Ctrl-C, leaving them running (native process manager only).
- `devenv processes start <name>` and `devenv up <name>` now share the same dependency-aware launch path. They honor `after`/`before` ordering; always start explicitly named processes, even with `processes.<name>.start.enable = false`; report unknown names with guidance; and register the full process set so other processes can be started later. When no manager is running, a named start launches one in the background, as with `devenv up -d <name>` ([#2930](https://github.com/cachix/devenv/issues/2930)).
- Process port listings now include ports derived from `listen` readiness probes, not just explicitly declared ports, in both the TUI and `devenv processes list`.

### Breaking Changes

- **Dropped `x86_64-darwin` (Intel macOS)**: devenv is no longer built, tested, or released for `x86_64-darwin`. Intel Mac users should pin an older devenv CLI release. `x86_64-darwin` environments can still be run via Rosetta 2 on Apple Silicon; however, nixpkgs 26.05 will be the last release supporting the platform.
- **Shell hook auto-activation**: The shell hook (`devenv hook bash`/`zsh`/`fish`/`nu`) and `devenv allow` now detect a project by looking for `devenv.nix` instead of `devenv.yaml`. Projects with only a `devenv.yaml` and no `devenv.nix` will no longer auto-activate; add a `devenv.nix` to restore activation.
- **`devenv init` no longer creates `.envrc` by default**: Pass `--include-envrc` or set `DEVENV_INCLUDE_ENVRC` to include the file ([#2859](https://github.com/cachix/devenv/pull/2859)).

## 2.1.2 (2026-05-13)

### Bug Fixes

- Fixed `devenv --profile <name> <subcommand>` failing when the profile name shadows a subcommand (e.g. `devenv --profile test test`). The profile value is now consumed before clap's subcommand precedence check ([#2821](https://github.com/cachix/devenv/issues/2821)).
- Fixed Nix syntax errors in `devenv.nix` being reported as unrelated warnings (e.g. `warning: Ignoring the client-specified setting 'system'...`) instead of the actual syntax error. Warning-prefixed log entries emitted before the failing operation no longer shadow the real diagnostic ([#2820](https://github.com/cachix/devenv/issues/2820)).
- Fixed the shell hook's "exit on cd-out" feature silently breaking under `clean.enabled = true`. `_DEVENV_HOOK_DIR` is now always preserved across env cleaning, so the hook-spawned shell still exits when the user `cd`s out of the project. Also aligned the fish hook's marker check with the posix one (non-empty value, not just "set").
- Fixed TUI panic ("attempt to read state after owner was dropped") when pressing Esc with content that fits in the viewport. The Esc handler now only scrolls when the ScrollView is actually mounted, matching the existing guard on up/down navigation.
- Fixed private Cachix caches failing with HTTP 401 on `nix-cache-info` after the v2.1 lazy Cachix refactor. `apply_store_settings` now sets the `netrc-file` global before adding substituters, so the authenticated probe Nix sends when registering a private substituter picks up the credentials written by the Cachix manager.
- Fixed `devenv hook <shell>` panicking with `failed printing to stdout: Broken pipe (os error 32)` when its downstream reader (e.g. `source` in `devenv hook fish | source`) closes the pipe before the script is fully flushed.

### Improvements

- Long log lines (structured logs, stack traces, etc.) in the TUI now wrap onto continuation rows instead of being truncated at the terminal width. Applies to both the inline log view (shown for selected/failed activities and `devenv up` processes) and the expanded log view, matching the default behavior of `less` and `journalctl`. Carriage returns are also stripped from captured log lines so pty-induced CRLF endings no longer move the cursor mid-row and erase rendered content ([#2818](https://github.com/cachix/devenv/issues/2818)).

## 2.1.1 (2026-05-12)

### Bug Fixes

- Fixed gaps in terminal scrollback during heavy output with the auto-reloading shell. The internal VT scrollback buffer is now larger and cleared after each flush to the native terminal, so its cap is never reached and lines are no longer dropped mid-stream due to GC ([#2810](https://github.com/cachix/devenv/issues/2810)).
- `$SHELL` is now set to the target shell before `enterShell` hooks run, so scripts that branch on `$SHELL` see the shell the user is actually entering.
- Fixed `zoxide: infinite loop detected` in the fish hook when `cd` is overridden to behave like `z` (e.g. `zoxide init --cmd=cd`) ([#2801](https://github.com/cachix/devenv/issues/2801)).
- Fixed `devenv --version` and `devenv -V` failing with `'devenv' requires a subcommand but one was not provided`. The flags now print the version and exit, matching the behavior of `devenv --help` ([#2791](https://github.com/cachix/devenv/issues/2791)).
- Fixed `devenv hook fish` not activating when starting a new fish shell directly inside a project directory. The initial activation now runs on the first `fish_prompt` event instead of inline during `source`, so the spawned `devenv shell` inherits the real terminal as stdin instead of the closed pipe from `devenv hook fish | source` ([#2798](https://github.com/cachix/devenv/issues/2798)).
- Fixed `devenv shell` skipping the user's `.zshenv` when launching zsh, which could mangle prompts and break `.zshrc` configurations that depend on env vars defined in `.zshenv`. The user's `.zshenv` is now sourced before `.zshrc` during shell init, matching zsh's documented startup order ([#2802](https://github.com/cachix/devenv/pull/2802)).
- Fixed the bash/zsh shell hook (`devenv hook bash`/`zsh`) re-launching `devenv shell` on every prompt redraw after the user interrupted a slow eval with Ctrl-C. The hook now caches `$PWD` before launching the subshell, so an aborted or failed activation no longer retries until the user `cd`s away and back.
- Fixed the shell hook closing the user's terminal when cd-ing out of a project directory while `DEVENV_ROOT` was exported into the outer shell (e.g. via direnv). The "exit on cd-out" path now keys off `_DEVENV_HOOK_DIR`, which is only set on shells the hook spawned, instead of `DEVENV_ROOT` alone ([#2805](https://github.com/cachix/devenv/issues/2805)).

### Improvements

- `devenv shell` now registers zsh completions from packages in the devenv profile. A generated `.zshenv` prepends `$DEVENV_PROFILE/share/zsh/site-functions` to `fpath` before `/etc/zshrc` runs, so the system `compinit` picks up the new directory.
- `devenv` now walks up parent directories to find `devenv.nix`, so commands like `devenv shell` work from any subdirectory of a project. The shell and `devenv shell -- <cmd>` still run from the directory you invoked them in, so relative paths resolve where you expect ([#2232](https://github.com/cachix/devenv/issues/2232)).
- Bumped `secretspec` to `v0.10.1`. The new `bws` (Bitwarden Secrets Manager) feature is not enabled because its transitive `bitwarden` crate conflicts with `sqlx` 0.8 on `libsqlite3-sys` and pins `typenum` to 1.18.
- Bumped `iocraft` to `0.8.2` and switched the `[patch.crates-io]` entry from the `cachix/iocraft` fork to upstream `ccbrown/iocraft` `main`, now that the row-level diff and stderr rendering patches are merged upstream.
- `devenv.yaml` options are now documented in `snake_case` (e.g. `allow_unfree`, `clean_env`). The previous `camelCase` spellings remain supported for backward compatibility.

## 2.1.0 (2026-05-05)

### Bug Fixes

- Fixed dynamic process port allocation reusing stale eval/build-cache entries without replaying port reservations, which could make parallel `devenv up` instances both try the same base port ([#2779](https://github.com/cachix/devenv/issues/2779)).
- Fixed `exec_if_modified` task checks walking the filesystem twice per run.
- Fixed `set: Tried to change the read-only variable "PWD"` and `"SHLVL"` errors in fish after `devenv shell` reloads.
- Fixed wrong background color in TUI apps (e.g. neovim) inside `devenv shell`. Cells the app cleared with a custom background now keep that color instead of falling back to the terminal default.
- Fixed substituter `nix-cache-info` downloads appearing at the top level by nesting them under "Configuring cachix".
- Fixed TUI rendering glitches during lock validation, and nested fetch activities (e.g. tarball downloads) under "Validating lock".
- Fixed TUI freezing while Cachix finishes uploading on shell entry. Push progress is now visible during cleanup.
- Fixed Cachix push progress getting stuck at `0/N`. Already-cached paths now count toward progress.
- Fixed tracked path log lines showing as dot-prefixed rows under "Evaluating shell" in the TUI instead of attaching to the eval activity.
- Fixed "suspicious ownership" errors when using Nix installed in single-user mode. Nix requires read-only group permissions on outputs ([#2751](https://github.com/cachix/devenv/issues/2751)).
- Fixed hot reload not watching files transitively imported by `devenv.nix` (e.g. `imports = [ ./nested/child.nix ]`).
- Fixed the root `devenv.nix` being imported twice when `devenv.yaml` imported a sub-project that also had its own `devenv.yaml` ([#2755](https://github.com/cachix/devenv/issues/2755)).
- Fixed `devenv shell` and `devenv build` failing with `path '...drv' is required, but there is no substituter that can build it` after the cached derivation was garbage-collected. The stale eval-cache entry is now invalidated when its referenced store paths no longer exist, forcing a re-evaluation that re-materializes the `.drv` on disk.
- Fixed hot reload missing file and directory changes that occurred during a build or during the brief gap between watch refreshes. The reload watcher now tracks path state (file/directory/missing) across reload cycles, queues deferred events while a build is in progress, and reconciles drift after rewatching so missed changes still trigger a follow-up rebuild.
- Fixed MCP server segfault on exit by waiting for the cache init thread to finish before exiting ([#2699](https://github.com/cachix/devenv/issues/2699)).
- Fixed independent one-shot tasks (e.g. `devenv:files` and `devenv:python:virtualenv`) running sequentially instead of in parallel, causing unnecessary waterfall delays during `devenv shell` startup.
- Fixed `nix build` failing with `E0463: can't find crate for devenv_reload` by switching the iocraft `[patch.crates-io]` entry to the `row-level-diff` branch, which carries iocraft 0.8.0. The previous `cachix` branch was stuck at 0.7.18 so the patch no longer applied, leaving two iocraft versions in the dependency graph and causing rustc metadata hash mismatches.
- Fixed `processes.<name>.watch` restarting processes multiple times for a single burst of queued file watcher events by draining the watch queue before restart ([#2735](https://github.com/cachix/devenv/pull/2735)).
- Fixed `processes.<name>.watch` restarting processes on read-only access, directory listing, and metadata-only file events ([#2734](https://github.com/cachix/devenv/pull/2734)).
- Fixed `imports` overriding the base project's `inputs` (e.g. `inputs.nixpkgs.url`) instead of the base config taking precedence ([#2728](https://github.com/cachix/devenv/issues/2728)).
- Fixed Boehm GC "Repeated allocation of very large block" warnings being printed to stderr during `devenv shell`.
- Fixed `devenv hook` not changing directory when `cd`ing out of a devenv project. The shell would deactivate but remain in the project directory instead of following the user to the target directory.
- Fixed `devenv hook fish` causing infinite recursion / hang when entering a devenv project, because the nested `fish -c` invocation re-sourced `~/.config/fish/config.fish` which re-ran the hook. The nested fish now uses `--no-config` ([#2741](https://github.com/cachix/devenv/issues/2741)).
- Fixed the Cachix daemon failing to start because the evaluated store path for the Cachix binary was never realized. It now uses the Cachix binary bundled with devenv via `PATH`, falling back to evaluating `cachix.binary` only when needed.
- Fixed warning messages from Nix not being forwarded and displayed during evaluation.
- Fixed `devenv test` leaving orphaned processes after test failures by ensuring processes are always stopped before propagating errors.
- Fixed adding a new input to `devenv.yaml` causing all existing inputs to be re-fetched instead of only resolving the new one ([#2688](https://github.com/cachix/devenv/issues/2688)).
- Fixed `processes.<name>.watch.paths` not triggering process restarts because Nix paths were serialized as store paths instead of source directory paths ([#2657](https://github.com/cachix/devenv/issues/2657)).
- Fixed cursor shape escape sequences (DECSCUSR) not being forwarded to the terminal in `devenv shell`, which caused programs like neovim to not change cursor shape between modes (e.g. block in normal mode, bar in insert mode).
- Fixed shift+mouse configuration (XTSHIFTESCAPE) not being forwarded to the terminal in `devenv shell`.
- Fixed `devenv shell` hanging indefinitely when exiting the shell during a hot-reload build by interrupting the lingering Nix evaluation on shutdown.
- Fixed `devenv repl` broken TUI output and hanging evaluation by adding proper TUI handoff support, showing evaluation progress in the TUI before handing the terminal to the interactive REPL.
- Fixed Ghostty shell integration not working in `devenv shell` by sourcing `ghostty.bash` from the rcfile when `GHOSTTY_RESOURCES_DIR` is set.
- Fixed TUI panic when expanding error details containing multi-byte UTF-8 characters (e.g. miette underlines) by using character-based truncation instead of byte-based slicing.
- Fixed `devenv:git-hooks:install` in linked worktrees and submodules by updating `rolling` to `prek 0.3.9`, which honors repo-local and worktree-local `core.hooksPath`.
- Fixed `devenv processes stop` removing the process from the manager state, making it impossible to start or restart afterwards.
- Fixed process exec probes failing with "No such file or directory" during `devenv test` when a task depends on a process (e.g. `after = [ "devenv:processes:postgres" ]`), because the bash path was not resolved for the enterTest task runner ([#2713](https://github.com/cachix/devenv/issues/2713)).
- Fixed processes with `restart = "never"` not satisfying `@completed` task dependencies, causing a hot loop and dependencies to never resolve ([#2712](https://github.com/cachix/devenv/issues/2712)).
- Fixed the TUI hiding Nix evaluation error details and showing only a failure mark without the actual error message. Failed "Evaluating" activities now automatically fetch and display their propagated error logs ([#2720](https://github.com/cachix/devenv/issues/2720)).

### Improvements

- The TUI now shuts down ~50ms faster.
- TUI operation activities (`Configuring shell`, `Configuring cachix`, `Loading tasks`, etc.) now nest under their parent activity instead of being forced to the top level. "Configuring cachix" appears as a child of "Configuring shell" (next to "Evaluating shell"), reflecting that Cachix setup runs as part of shell configuration.
- "Validating lock" is now visible in the TUI by default instead of hidden behind the Debug filter.
- Sped up `devenv shell` startup on projects with many cached input paths by batching file watcher registration into a single pathset update and readiness wait, instead of reconciling the pathset once per path. This removes long hangs before `enterShell` on large inputs.
- Fixed port allocation values (`config.processes.<name>.ports.<port>.value`) resolving to the base `allocate` port in `devenv shell`, `devenv tasks run`, and other commands. When the native process manager is running, port values now match the ports allocated by `devenv up` ([#2710](https://github.com/cachix/devenv/issues/2710)).
- Standardized keyboard shortcut notation across `devenv up` TUI and `devenv shell` to use consistent `Ctrl-E` format instead of mixed `^e`/`Ctrl-Alt-E` styles. macOS now shows `Opt` instead of `Alt` ([#2736](https://github.com/cachix/devenv/issues/2736)).
- Auto-detect AI coding agents (via `CLAUDECODE`, `OPENCODE_CLIENT`, and `AI_AGENT` environment variables) and enable quiet mode to avoid wasting LLM tokens on TUI progress output. Override with `--verbose`, `--tui`, or set `DEVENV_NO_AI_AGENT=1` to disable detection entirely ([#2723](https://github.com/cachix/devenv/issues/2723)).
- Always enable Nix's `show-trace` setting so full evaluation stack traces are shown on error, instead of a truncated trace suggesting the nonexistent `--show-trace` flag ([#2725](https://github.com/cachix/devenv/issues/2725)).
- Upgraded Nix to 2.34, bringing multithreaded tarball unpacking, evaluator performance improvements, and REPL enhancements.
- Added `require_version` field to `devenv.yaml` to enforce a devenv CLI version. Set to `true` to match the modules version, or use a constraint string like `">=2.1"` ([#2391](https://github.com/cachix/devenv/issues/2391)).
- Added Ctrl-X keybinding to stop individual processes from the TUI while keeping them visible and restartable.
- Added Ctrl-H keybinding to toggle hiding stopped processes in the TUI. Failed processes remain visible, and the process count shows how many are hidden ([#2692](https://github.com/cachix/devenv/issues/2692)).
- Tasks can now display messages when entering the shell by writing `{"devenv":{"messages":["..."]}}` to `$DEVENV_TASK_OUTPUT_FILE` ([#2500](https://github.com/cachix/devenv/issues/2500)).
- Added `devenv hook <shell>` for native directory based auto-activation without direnv. Supports bash, zsh, fish, and nushell. Automatically deactivates when you leave the project directory. Add `eval "$(devenv hook bash)"` to your shell config to activate. Use `devenv allow` and `devenv revoke` to manage trust ([#2488](https://github.com/cachix/devenv/issues/2488)).
- Shell environment now auto-reloads at the next prompt when watched files change, instead of requiring a manual Ctrl-Alt-R keybind ([#2595](https://github.com/cachix/devenv/issues/2595)).
- Added `nixpkgs.rocmSupport` option to enable ROCm support in nixpkgs configuration.
- Added process management subcommands and MCP tools: `devenv processes list`, `status`, `logs`, `restart`, `start`, `stop` for interacting with running native processes ([#2621](https://github.com/cachix/devenv/issues/2621)).
- Added `--mode` flag to `devenv up` / `devenv processes up` to control dependency resolution for process tasks. Supports `single`, `before`, `after`, and `all` modes, matching `devenv tasks run --mode`. Defaults to `all`, so `devenv up` starts all processes by default ([#2721](https://github.com/cachix/devenv/issues/2721)).
- Added OpenTelemetry OTLP trace export support via `--trace-to` (`otlp-grpc`, `otlp-http-protobuf`, `otlp-http-json`) with configurable endpoints or standard `OTEL_*` environment variables. OTLP spans include rich attributes (`devenv.activity.kind`, `devenv.derivation_path`, `devenv.url`, `devenv.fetch.kind`) and outcome tracking (`devenv.outcome`, `otel.status_code`). Enabled by default with the `otlp-grpc` cargo feature; `otlp-http-protobuf` and `otlp-http-json` are opt-in ([#2415](https://github.com/cachix/devenv/issues/2415)).
- Added OTEL trace context propagation to subprocesses: tasks, shell commands, and processes automatically receive `TRACEPARENT`/`TRACESTATE` environment variables when OTLP export is enabled, allowing instrumented child processes to join the same trace ([#2415](https://github.com/cachix/devenv/issues/2415)).
- Added `--trace-to` unified tracing flag with `[format:]destination` syntax, supporting multiple simultaneous outputs (e.g. `--trace-to pretty:stderr --trace-to json:file:/tmp/trace.json`). Also available as `DEVENV_TRACE_TO` env var (comma-separated). Legacy `--trace-output`/`--trace-format` flags are still supported but hidden.

### Breaking Changes

- **`devenv tasks run`**: The default execution mode is now `before` instead of `single`, so task dependencies declared via `before`/`after` are respected by default. Running `devenv tasks run admin:deploy` now also runs any tasks that `admin:deploy` depends on. Use `--mode single` to restore the previous behavior of running only the specified task ([#2551](https://github.com/cachix/devenv/issues/2551)).
- Calling `devenv` without a command now shows the help text. This used to print the version. Use `devenv version` or `devenv --version` instead.

## 2.0.6 (2026-03-22)

### Bug Fixes

- Fixed task cache initialization failing with "unable to open database file" when the state directory does not yet exist.
- Fixed Cachix daemon log output leaking into and corrupting the TUI display by capturing daemon stderr and forwarding it through the push activity ([#2648](https://github.com/cachix/devenv/issues/2648)).
- Fixed `devenv test` hanging when a process outputs non-UTF-8 bytes by using lossy UTF-8 decoding instead of closing the pipe, which caused a deadlock between the parent waiting for the child to exit and the child blocking on a full stdout pipe ([#2590](https://github.com/cachix/devenv/issues/2590)).
- Fixed hot reload sometimes picking up stale configuration or crashing due to the old Nix evaluator not being fully cleaned up before creating a new one. Errors during reload are now reported instead of silently breaking subsequent evaluations.
- Fixed `DEVENV_TUI=false` and `DEVENV_TUI=0` not disabling the TUI ([#2646](https://github.com/cachix/devenv/issues/2646)).
- Migrated `certificates` and `hosts` modules to use tasks instead of `process.manager.before`/`process.manager.after` for the native process manager ([#2569](https://github.com/cachix/devenv/issues/2569)).
- Fixed stale shell eval cache entries causing `devenv shell` and `direnv` activation to ignore changes to `devenv.nix` config such as `languages`, `env`, `packages`, and `scripts` ([#2643](https://github.com/cachix/devenv/pull/2643)).
- Fixed services failing to start with E2BIG when the shell environment is large by moving env/cwd/stdio setup from the bash wrapper script to the spawned process directly ([#2638](https://github.com/cachix/devenv/issues/2638)).
- Fixed processes failing to start when exported bash functions are present in the environment ([#2587](https://github.com/cachix/devenv/issues/2587)).
- Fixed eval cache storing inconsistent port allocations across different cached attributes ([#2631](https://github.com/cachix/devenv/issues/2631)).
- Fixed stale eval cache invalidation for `devenv up` process config changes caused by overlapping evaluations clearing each other's file dependency observers ([#2632](https://github.com/cachix/devenv/pull/2632)).
- Fixed strict port restarts failing with "port already in use" when the kernel hasn't released the socket yet after process shutdown, by distinguishing ownerless transient conflicts from real ones and retrying accordingly ([#2647](https://github.com/cachix/devenv/pull/2647)).
- Fixed false-positive `--strict-ports` failures on macOS when IPv6 loopback binding returns a benign error after a clean shutdown and cache invalidation ([#2640](https://github.com/cachix/devenv/pull/2640)).
- Fixed child processes being left running on shutdown when using non-native process managers like process-compose ([#2586](https://github.com/cachix/devenv/issues/2586)).
- Fixed `devenv update` resolving stale revisions when Nix's fetcher cache contains outdated entries by setting `tarball-ttl` to 0 during update, equivalent to `nix --refresh` ([#2616](https://github.com/cachix/devenv/issues/2616)).
- Fixed Nix backend initialization crash when `impure: true` by removing use of nonexistent `impure` Nix setting; impure mode now works by skipping `pure-eval` (which defaults to false).
- Fixed `execIfModified` glob walker entering gitignored directories, causing extreme slowdowns in repos with large ignored trees ([#2588](https://github.com/cachix/devenv/issues/2588)).
- Fixed `devenv up -d` not keeping processes running with the native process manager by spawning a daemon via re-exec instead of fork, which is unsafe in multithreaded programs ([#2630](https://github.com/cachix/devenv/issues/2630)).
- Fixed environment variable precedence in process manager: per-process env now correctly wins over global env.
- Fixed hot reload triggering a fork bomb by ignoring file change events while a build is already in progress, preventing cascading zombie builds.

### Improvements

- The TUI now shows a "stopping" status during graceful process shutdown ([#2647](https://github.com/cachix/devenv/pull/2647)).
- Process `stop_all()` now stops processes concurrently instead of serially ([#2647](https://github.com/cachix/devenv/pull/2647)).
- `--quiet` now disables the TUI and suppresses all output except warnings and errors ([#2617](https://github.com/cachix/devenv/issues/2617)).
- Bumped Nix input to fix `devenv shell` failing with "suspicious ownership or permission" on single user Nix installations where umask causes store outputs to have incorrect permissions ([#2585](https://github.com/cachix/devenv/issues/2585)).
- Bumped iocraft to use row level diff rendering ([ccbrown/iocraft#179](https://github.com/ccbrown/iocraft/pull/179)), reducing TUI flicker.
- Containers now report all missing inputs at once instead of one at a time ([#2598](https://github.com/cachix/devenv/issues/2598)).
- Extracted container, search, and gc methods from `devenv.rs` into separate submodule files for better code organization.
- Added `allowUnsupportedSystem` option for `devenv.yaml` to allow packages not supported on the current system. Can be set at the top level, under `nixpkgs`, or per platform via `nixpkgs.per-platform.<system>` ([#2639](https://github.com/cachix/devenv/pull/2639)).
- Added new nixpkgs config options to `devenv.yaml`: `allowNonSource`, `allowlistedLicenses`, `blocklistedLicenses`, and `androidSdk.acceptLicense` ([#2634](https://github.com/cachix/devenv/issues/2634)).
- Added `allowInsecurePredicate` generation from `permittedInsecurePackages` so insecure package allowlisting works correctly.

### Breaking Changes

- **`process.manager.before`/`process.manager.after`**: These options are no longer supported with the native process manager. Use tasks with `before`/`after` dependencies instead. See the [tasks documentation](https://devenv.sh/tasks/).

## 2.0.5 (2026-03-16)

### Improvements

- Show a quit prompt on the first Ctrl-C in the TUI instead of immediately terminating ([#2607](https://github.com/cachix/devenv/pull/2607)).
- Patched Nix to avoid hitting GitHub rate limits when fetching flake inputs (upstreamed as [NixOS/nix#15470](https://github.com/NixOS/nix/pull/15470)).
- Improved eval performance by caching the initial Nix Value, avoiding re-evaluation of nixpkgs and the module system on subsequent attribute lookups (~2x time-to-shell improvement).
- `devenv-run-tests`: `--only` and `--exclude` now support glob patterns (e.g. `--only 'python-*'`).
- Added Fish and Nushell support for interactive devenv sessions (`--shell fish`, `--shell nu`).

### Bug Fixes

- Fixed `devenv test` not running `enterTest` tasks (e.g., `git-hooks:run`) in devenv 2.0+. Also: `devenv test` now fails early when enterTest tasks fail, and skips redundant `load_tasks()` when tasks are already handled.
- Fixed file watcher dropping change events during the initial bootstrap file flood by switching from `try_send` to backpressure, which caused `devenv.nix` changes to go undetected during hot reload.
- Fixed `exec_if_modified` performance when negation patterns were used, avoiding a full walk of the parent directory for literal file paths.
- Fixed child processes (postgres, redis, etc.) being left running after `devenv up` exits or `devenv processes down` is called. The native manager wrapper now forwards TERM/INT signals to the child process group, and the process-compose backend creates a proper process group for signaling ([#2619](https://github.com/cachix/devenv/issues/2619)).
- Fixed SecretSpec prompting for secrets in non-interactive contexts like direnv.
- Fixed `devenv search` showing truncated package names (e.g. `pkgs.` instead of `pkgs.ncdu`).
- Fixed runtime directory path (`devenv-<hash>`) being inconsistent on macOS when paths contain symlinks (e.g. `/tmp` vs `/private/tmp`), which could cause processes to look for sockets in the wrong directory.
- Fixed TUI hanging when the backend encounters an error in the PTY shell path (e.g. Nix evaluation failure).
- Fixed `nix run` trying to run `devenv-wrapped` which doesn't exist.
- Fixed in-band resize events being sent to the shell when the app did not opt-in to receiving them.
- Fixed a nixpkgs packaging error that caused macOS builds of devenv to include two conflicting copies of the Boehm GC ([#2552](https://github.com/cachix/devenv/issues/2552), [#2576](https://github.com/cachix/devenv/issues/2576)).
- Fixed `devenv init`, `devenv test`, and SecretSpec hint messages being silently dropped due to a missing user-message marker.

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
- Bumped SecretSpec to 0.8.0 and enabled all provider features (Google Cloud Secret Manager, AWS Secrets Manager, HashiCorp Vault).
- Replaced the stdout-based `DEVENV_EXPORT:` protocol in tasks with file-based exports (`$DEVENV_TASK_EXPORTS_FILE`), simplifying the encoding and moving JSON construction into Rust.
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
- Fixed TUI overflow being sent to scrollback instead of being clipped.
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
- Expandable log views for activities and processes (Ctrl-E to expand).
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
- Ctrl-Alt-D shortcut to pause/resume file watching.
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
- Fixed terminal hang on Ctrl-C during shell building.
- Fixed missing prompt after task execution in shell.
- Fixed relative path inputs with `..` components.
- Fixed `devenv.local.nix` loading from imported directories.
- Fixed `devenv.cli.version` allowing null for flakes integration.
- Fixed `TMPDIR` separation from `DEVENV_RUNTIME` to avoid polluting `XDG_RUNTIME_DIR`.
- Fixed PATH preservation in shell and direnv environments.
- Fixed cursor position preservation after Ctrl-L clear in shell.
- Fixed eval cache handling of percent-encoded characters in database paths.
- Fixed changelog generation when assemble fails during `devenv update`.
- Fixed profile state isolation in separate directories.
- Fixed processes preventing infinite recursion when conditionally defining processes.
- Fixed load-exports permission error when file is owned by a different user.
- Fixed `-O packages:pkgs` replacing all packages instead of appending.
  The `pkgs` type now appends to existing packages by default.
  Use `pkgs!` to force-replace the entire list.
