---
date: 2026-05-07
authors:
  - domenkozar
draft: false
---

# devenv 2.1: Nix with zsh, fish, and nushell via libghostty

[devenv 2.0](devenv-v2.0-a-fresh-interface-to-nix.md) gave you hot reload, the status line, and instant cache hits, but `devenv shell` always dropped you into bash, and you still needed `direnv` for activation on `cd`. 

[devenv 2.1](https://github.com/cachix/devenv/releases/tag/v2.1) closes both gaps and adds structured handles for coding agents.

## Every shell, first class

### Native zsh, fish, and nushell

![Shell reloading in zsh](../../assets/images/devenv-2.1-shell-reloading-zsh.gif)

[devenv 2.1](https://github.com/cachix/devenv/releases/tag/v2.1) adds native support for **zsh**, **fish**, and **nushell** ([devenv#2718](https://github.com/cachix/devenv/pull/2718)) with rcfile generation, environment diff tracking, reload hooks, and prompt integration implemented per shell rather than shimmed through bash. The shell is picked from `$SHELL`, or set explicitly:

```shell-session
$ devenv shell
$ SHELL=/bin/zsh devenv shell
```

Closes [devenv#36](https://github.com/cachix/devenv/issues/36) (open since November 2022), [devenv#2487](https://github.com/cachix/devenv/issues/2487), and [devenv#2592](https://github.com/cachix/devenv/issues/2592).

### libghostty under the hood

The virtual terminal emulator was replaced with [libghostty](https://github.com/niclas-ahden/libghostty-rs), the terminal engine from [Ghostty](https://ghostty.org/), giving devenv a single VT parser that handles every shell the same way.

Building libghostty reliably on Nix took upstream patches in [libghostty-rs#27](https://github.com/Uzaaft/libghostty-rs/pull/27), [ghostty#12364](https://github.com/ghostty-org/ghostty/pull/12364), and [ghostty#12548](https://github.com/ghostty-org/ghostty/pull/12548). Thanks to the Ghostty maintainers for landing them.

### Auto reload

2.0 required `Ctrl+Alt+R` to apply environment changes after a rebuild, and that keybind clashed with reverse search on macOS. [2.1](https://github.com/cachix/devenv/releases/tag/v2.1) re evaluates in the background on file changes and applies the new environment at the next prompt ([devenv#2595](https://github.com/cachix/devenv/issues/2595)).

## Auto activation without direnv

[`devenv hook`](../../auto-activation.md) replaces direnv for `cd` based activation. Add one line to your shell config:

=== "Bash"

    ```bash title="~/.bashrc"
    eval "$(devenv hook bash)"
    ```

=== "Zsh"

    ```bash title="~/.zshrc"
    eval "$(devenv hook zsh)"
    ```

=== "Fish"

    ```fish title="~/.config/fish/config.fish"
    devenv hook fish | source
    ```

=== "Nushell"

    ```nu title="config.nu"
    devenv hook nu | save --force ~/.cache/devenv/hook.nu
    source ~/.cache/devenv/hook.nu
    ```

Activation happens on `cd` into a trusted directory and reverses on the way out. No `.envrc`, no external dependencies. Trust is managed with `devenv allow` and `devenv revoke`.

## For coding agents

In 2.0, an agent that wanted to restart your API after a config change had to kill the whole devenv session or scrape the TUI through ANSI codes. [2.1](https://github.com/cachix/devenv/releases/tag/v2.1) replaces that with structured handles.

### Process management from the command line

New subcommands act on a running `devenv up` ([devenv#2621](https://github.com/cachix/devenv/issues/2621)):

```shell-session
$ devenv processes list
$ devenv processes status
$ devenv processes logs api
$ devenv processes restart api
$ devenv processes stop worker
$ devenv processes start worker
```

These work with the native process manager and are also exposed as MCP tools.

### Quiet mode by default

devenv detects agents via `CLAUDECODE`, `OPENCODE_CLIENT`, and `AI_AGENT` and switches to quiet mode automatically, suppressing TUI progress output that would waste tokens ([devenv#2723](https://github.com/cachix/devenv/issues/2723)). Override with `--verbose` or `--tui`.

## OpenTelemetry trace export

[devenv 2.1](https://github.com/cachix/devenv/releases/tag/v2.1) exports OTLP traces ([devenv#2415](https://github.com/cachix/devenv/issues/2415)). Every Nix evaluation, derivation build, task run, and managed process becomes a span with attributes like `devenv.activity.kind`, `devenv.derivation_path`, `devenv.url`, and `devenv.outcome`.

Enable it through the new unified `--trace-to` flag:

```shell-session
$ devenv --trace-to otlp-grpc shell
$ devenv --trace-to otlp-http-protobuf:http://localhost:4318 shell
```

Three OTLP formats are supported: `otlp-grpc` (built in), `otlp-http-protobuf`, and `otlp-http-json` (opt in via cargo features). Endpoints can be set on the flag or via the standard `OTEL_EXPORTER_OTLP_*` variables.

Trace context propagates across process boundaries: spawned tasks, shell commands, and processes inherit `TRACEPARENT` and `TRACESTATE`, so instrumented children show up on the same trace as the parent `devenv up` run.

`--trace-to` replaces `--trace-output` and `--trace-format` with a single `[format:]destination` syntax, and accepts multiple destinations:

```shell-session
$ devenv --trace-to pretty:stderr --trace-to otlp-grpc shell
$ DEVENV_TRACE_TO=json:file:/tmp/trace.json,otlp-grpc devenv shell
```

## Tasks and processes

`devenv tasks run` defaults to `before` mode, so dependencies run too ([devenv#2551](https://github.com/cachix/devenv/issues/2551)); `--mode single` restores the old behavior. The same `--mode` flag now also controls which processes `devenv up` starts ([devenv#2721](https://github.com/cachix/devenv/issues/2721)).

Tasks can print messages on shell entry by writing to `$DEVENV_TASK_OUTPUT_FILE` ([devenv#2500](https://github.com/cachix/devenv/issues/2500)).

## And more

**Nix 2.34.** Multithreaded tarball unpacking, evaluator performance improvements, and REPL enhancements.

**`require_version` in devenv.yaml.** Enforce a minimum devenv CLI version for your project. Set `require_version: true` to match the modules version, or use a constraint string like `">=2.1"` ([devenv#2391](https://github.com/cachix/devenv/issues/2391)).

**ROCm support.** New `nixpkgs.rocmSupport` option for enabling ROCm in nixpkgs configuration.

**Full stack traces on error.** `show-trace` is now always enabled, so evaluation errors include the full stack trace instead of a truncated message suggesting a nonexistent `--show-trace` flag ([devenv#2725](https://github.com/cachix/devenv/issues/2725)).

**Ctrl+X to stop processes.** Stop individual processes from the TUI while keeping them visible and restartable.

**Ctrl+H to hide stopped processes.** Toggle hiding stopped processes in the TUI to focus on what's still running. Failed processes stay visible, and the process count shows how many are hidden ([devenv#2692](https://github.com/cachix/devenv/issues/2692)).

**Port allocation fixes.** Port values (`config.processes.<name>.ports.<port>.value`) now resolve correctly in `devenv shell` and `devenv tasks run`, matching the ports allocated by `devenv up` ([devenv#2710](https://github.com/cachix/devenv/issues/2710)). Ports bound to `0.0.0.0` or `[::]` are now detected, preventing multiple devenv instances from allocating the same port ([devenv#2567](https://github.com/cachix/devenv/issues/2567)). Strict port restarts no longer fail with "port already in use" during kernel socket teardown ([devenv#2647](https://github.com/cachix/devenv/pull/2647)).

**Dozens of other bug fixes.** File watcher deduplication, import precedence, eval cache consistency, process lifecycle fixes, and terminal compatibility improvements. See the full [changelog](https://github.com/cachix/devenv/blob/main/CHANGELOG.md) for details.

## Breaking changes

- **`devenv tasks run`** now runs dependencies by default (`before` mode instead of `single`). Use `--mode single` for the old behavior.

## Final words

[Open an issue](https://github.com/cachix/devenv/issues) or join the [Discord](https://discord.gg/naMgvexb6q) with feedback.

Domen
