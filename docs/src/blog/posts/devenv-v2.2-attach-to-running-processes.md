---
date: 2026-07-02
authors:
  - domenkozar
draft: true
---

# devenv 2.2: attach to running processes and persistent out-of-tree environments

[devenv 2.2](https://github.com/cachix/devenv/releases#release-v2.2) ships:

- **[Attach to running processes](#one-process-manager-many-terminals):** share one native process manager across terminals, with live status, ports, logs, and a choice to detach or stop.
- **[Persistent out-of-tree environments](#configure-an-environment-once-use-it-anywhere):** bind a directory to a local or remote `--from` source, including profiles and the source's complete `devenv.yaml`.
- **[More reliable shell activation](#shell-activation-gets-out-of-the-way):** automatically load hooks in fish and nushell, select the correct login shell in stripped environments, and behave predictably in multiplexers and nested shells.
- **[SecretSpec 0.17 and Cachix integration](#secretspec-017-and-cachix-integration):** upgrade from SecretSpec 0.8, pull and push to private caches without exporting `CACHIX_AUTH_TOKEN`, and run the bundled SecretSpec CLI with the same profile and provider as devenv.
- **[A calmer and sturdier TUI](#tui-calmer-sturdier):** use near-zero CPU while idle, distinguish every process state at a glance, and handle unusual terminal input safely.
- **[Evaluation cache fixes](#evaluation-cache-fixes):** invalidate correctly when local inputs, copied sources, and files modified during evaluation change.
- **[Better automation interfaces](#for-automation-and-coding-agents):** inspect tasks as JSON, access inputs in the REPL, distinguish trace callers, and avoid streaming interactive output into coding agents.
- **[Smaller installs and better diagnostics](#smaller-installs-and-better-diagnostics):** remove unnecessary LLVM and debug-toolchain dependencies while surfacing clearer errors in terminals and CI.

## One process manager, many terminals

Running `devenv up` while the native process manager is already active now **attaches** to it instead of failing ([devenv#2936](https://github.com/cachix/devenv/pull/2936)). The second invocation connects over the control socket, starts any requested processes that aren't running yet, and streams status, ports, and logs.

```shell-session
$ devenv up -d                      # start processes in the background
$ devenv up                         # attach: live status, ports, and logs
$ devenv processes attach           # watch without requesting any starts
$ devenv processes start postgres   # start one process, cold-starting a manager if needed
```

The manager resolves `before` and `after` ordering through its own task scheduler. An attached terminal first receives the current state and recent log output, then follows live changes. Other terminals can attach and detach independently without taking ownership away from the daemon.

Ctrl-C on an attached `devenv up` asks whether you want to detach and leave the processes running, or stop the whole manager. `devenv processes attach` always detaches on Ctrl-C.

`devenv processes start <name>` no longer requires a manager to be running. It can cold-start one in the background with that process and its dependency closure, equivalent to `devenv up -d <name>` ([devenv#2930](https://github.com/cachix/devenv/issues/2930)). Named starts always start the requested process, even when its default `start.enable` is false.

Attaching is deliberately interactive. CI jobs, piped commands, and detected coding agents report that processes are already running instead of entering a live view that could wait indefinitely.

Under the hood, the process manager became the single owner of process state, the daemon pushes attach sessions as an event stream instead of being polled, and every process launch goes through the same task graph. This fixed a family of lifecycle bugs along the way: double launches, processes vanishing from `list`, stuck relaunches, concurrent daemon startup, and a foreground `devenv up` corrupting a running daemon's PID file.

Closes [devenv#971](https://github.com/cachix/devenv/issues/971), open since February 2024.

## Configure an environment once, use it anywhere

[`--from`](../../ad-hoc-developer-environments.md) lets you use a devenv without checking `devenv.nix` into the project. In 2.1 you had to repeat the flag for every command. In 2.2, `devenv allow` can bind the current directory to that source:

```shell-session
$ cd my-project
$ devenv --from github:myorg/devenv-configs?dir=rust-web allow
$ devenv shell
```

Every subsequent devenv command in that directory loads the bound configuration. The native shell hook auto activates it on `cd`, just like a project with a local `devenv.nix`.

Profiles persist with the binding:

```shell-session
$ devenv --from github:myorg/devenv-configs \
    --profile backend \
    --profile observability \
    allow
```

An explicit `--profile` still takes priority when you need a one-off override.

Local sources now bring their entire configuration with them. `--from path:../shared-devenv` loads the source's `devenv.yaml`, inputs, imports, and sibling modules from its Git repository. Modules are read from the live directory, so edits take effect without fetching or rebinding the source.

Normal in-tree projects are easier to navigate too: devenv now walks up parent directories to find `devenv.nix`. Commands work from any subdirectory while the shell and `devenv shell -- <command>` keep the directory where you invoked them ([devenv#2232](https://github.com/cachix/devenv/issues/2232)).

## Shell activation gets out of the way

The native shell integration added in 2.1 now loads automatically in fish and nushell through their vendor configuration mechanisms. Bash and zsh have no equivalent, so they still need the one-line [`devenv hook`](../../auto-activation.md) setup in your shell configuration.

Shell selection is more reliable in editor terminals and stripped environments where `$SHELL` is missing. devenv looks up the user's login shell before falling back to bash, and warns when a shell explicitly requested through `--shell` or `devenv.yaml` is unsupported ([devenv#2880](https://github.com/cachix/devenv/issues/2880), [devenv#2992](https://github.com/cachix/devenv/pull/2992)).

Activation and deactivation have also been hardened across real-world shell setups:

- New tmux and zellij panes, SSH sessions, and nested shells no longer inherit a marker that can close them when they change directory ([devenv#2861](https://github.com/cachix/devenv/issues/2861)).
- Leaving and immediately re-entering a project activates it again on the first try.
- Fish preserves `cd -` history and works when zoxide overrides `cd` ([devenv#2853](https://github.com/cachix/devenv/issues/2853)).
- A manually entered `devenv shell`, or an environment already loaded by direnv, no longer gets another devenv shell stacked on top.
- Nushell now deactivates reliably when leaving a project.

If you still use direnv, `devenv init --include-envrc` (or `DEVENV_INCLUDE_ENVRC`) adds an `.envrc` to a new project. Native activation remains the default and requires no project-local activation file.

## SecretSpec 0.17 and Cachix integration

devenv 2.2 upgrades its SecretSpec integration from 0.8 to [SecretSpec 0.17](https://secretspec.dev/blog/secretspec-0-17-scopes-secrets-caching-age-and-systemd-credentials/). The release adds scopes, secrets caching, cross-secret validation, GitHub and Forgejo Actions support, and new providers including SOPS, age, KeePass KDBX, OpenBao, Scaleway Secret Manager, and systemd credentials.

Pulling from and pushing to private Cachix caches no longer requires exporting `CACHIX_AUTH_TOKEN`. devenv can resolve the token through [SecretSpec](https://secretspec.dev) without exposing it to the development shell:

```yaml title="devenv.yaml"
secretspec:
  enable: true
  provider: keyring
  cachix_auth_token: true
```

No declaration in `secretspec.toml` is required. If SecretSpec does not return a token, devenv falls back to the token stored by the Cachix CLI through `cachix authtoken`. The resolved token is used for both pulls and the Cachix push daemon.

If your secrets backend grants access to the token under a different name, configure it in `devenv.yaml`:

```yaml title="devenv.yaml"
secretspec:
  enable: true
  cachix_auth_token: CI_CACHIX_TOKEN
```

The string is the secret's name, not the token itself.

The matching `secretspec` command is now bundled with devenv. The profile and provider selected in `devenv.yaml` are exported as `SECRETSPEC_PROFILE` and `SECRETSPEC_PROVIDER`, so runtime commands use the same configuration that devenv used during evaluation:

```shell-session
$ devenv shell
$ secretspec run -- npm start
```

## TUI: calmer, sturdier

**Process status at a glance.** Each process now shows a status dot whose shape encodes its lifecycle instead of an identical spinner on every row. Waiting, starting, running, ready, stopped, exited, failed, and gave-up states remain distinct. The shape carries the state, so it reads without relying on color.

**Near zero idle CPU.** `devenv up` was burning roughly 10 to 15% CPU per managed process even when everything was quiet, because the UI recomputed its layout dozens of times per second. The TUI now only redraws when something actually changes ([devenv#2915](https://github.com/cachix/devenv/issues/2915)).

**Fuzzed nightly.** The TUI gained property-based tests and a nightly terminal fuzzing run, which already flushed out crashes on multi-byte characters, progress overflow, and narrow terminals.

**Better terminal rendering.** Long structured logs and stack traces wrap instead of being truncated. Shell output is read in larger batches, reducing rendering overhead, and long lines copied from `devenv shell` no longer acquire a hard newline at the terminal's wrap point ([devenv#2865](https://github.com/cachix/devenv/issues/2865)).

## Evaluation cache fixes

Local `path:` inputs now participate in cache invalidation. Editing a shared configuration applies on the next command instead of returning stale results until `.devenv` is deleted.

The cache also tracks local files and directories copied into the Nix store during evaluation. Directories are hashed recursively, so changes to nested scripts and imported source trees invalidate the attributes that depend on them ([devenv#2886](https://github.com/cachix/devenv/issues/2886), [devenv#2893](https://github.com/cachix/devenv/issues/2893)).

If a tracked file changes while an evaluation is still running, that result is no longer stored. This closes a race where editing `devenv.nix` at the wrong moment could leave stale tasks or options behind ([devenv#2745](https://github.com/cachix/devenv/issues/2745)).

Other fixes cover newly created files with sub-second timestamps, SQLite caches on VM-mounted filesystems, and shells built through relocated or chrooted Nix stores ([devenv#2499](https://github.com/cachix/devenv/issues/2499), [devenv#2947](https://github.com/cachix/devenv/issues/2947)).

## For automation and coding agents

**`devenv down`.** A shorthand for `devenv processes down`, mirroring `devenv up` ([devenv#2862](https://github.com/cachix/devenv/issues/2862)).

**`devenv tasks list --json`.** Machine readable task graph inspection ([devenv#2966](https://github.com/cachix/devenv/issues/2966)).

**`inputs` in the REPL.** `devenv repl` now exposes `inputs` alongside `devenv` and `pkgs`, so you can poke at inputs declared in `devenv.yaml` directly (e.g. `inputs.nixpkgs.lib.version`).

**Better agent detection.** Quiet mode now kicks in for more coding agents (Aider, autonomous and cloud agents, and others) via the `detect-coding-agent` crate, not just Claude Code. Set `DEVENV_NO_AI_AGENT=1` to opt out.

**Trace callers.** OpenTelemetry spans now identify whether devenv was invoked by the CLI, direnv, or the native shell hook through the `devenv.caller` attribute ([devenv#2965](https://github.com/cachix/devenv/issues/2965)). `DEVENV_TRACE_DEFAULT_TO` configures a default trace destination without overriding an explicit `--trace-to` or `DEVENV_TRACE_TO`.

## Smaller installs and better diagnostics

Statically linking `nixd`, used by `devenv lsp`, removes the monolithic LLVM shared library and reduces the devenv closure and container image by roughly 550 MiB. Building libghostty-vt in release mode also avoids pulling a debug toolchain containing Zig and LLVM into every installation.

Non-TUI output now shows useful Nix evaluation and build progress while hiding internal debug noise. Evaluation warnings no longer replace the real error message, `devenv test --no-tui` preserves test output in CI, and unfree-package errors point to the relevant devenv configuration instead of generic NixOS advice.

`devenv shell` also terminates its inner shell when the terminal closes instead of leaving a background process at 100% CPU ([devenv#2845](https://github.com/cachix/devenv/issues/2845)). GitHub inputs resolve over SSH when a `url.insteadOf` rule rewrites them, using your SSH agent ([devenv#2842](https://github.com/cachix/devenv/issues/2842)).

See the full [changelog](https://github.com/cachix/devenv/blob/main/CHANGELOG.md) for the rest.

## Breaking changes

- **Dropped `x86_64-darwin` (Intel macOS).** devenv is no longer built, tested, or released for Intel Macs. Pin an older devenv release if you're on one. `x86_64-darwin` environments still run through Rosetta 2 on Apple Silicon, though nixpkgs 26.05 will be its final supported nixpkgs release.
- **Auto activation detects `devenv.nix`.** The shell hook and `devenv allow` now look for `devenv.nix` instead of `devenv.yaml`. Projects with only a `devenv.yaml` no longer auto activate; add a `devenv.nix` to restore activation.

## Final words

[Open an issue](https://github.com/cachix/devenv/issues) or join the [Discord](https://discord.gg/naMgvexb6q) with feedback.

Domen
