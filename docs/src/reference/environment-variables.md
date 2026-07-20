# Environment variables

devenv defines and respects the following environment variables:

- [devenv variables](#devenv-variables) are defined by devenv.
  The read-only ones are exported into the developer shell so you can reference them from scripts, [tasks](../tasks.md), and modules.
  The configurable ones are read by the CLI to change its behavior, and each mirrors a command-line flag that takes precedence.
- [Externally defined variables](#externally-defined-variables) are standard, third-party variables that devenv respects but does not define.

## devenv variables

### [`DEVENV_ROOT`](#devenv_root)
<small class="added-in">added in `0.2`</small>

**Read-only.** Points to the root of the project where `devenv.nix` is located.

### [`DEVENV_DOTFILE`](#devenv_dotfile)
<small class="added-in">added in `0.1`</small>

**Read-only.** Points to `$DEVENV_ROOT/.devenv`.

### [`DEVENV_STATE`](#devenv_state)
<small class="added-in">added in `0.1`</small>

**Read-only.** Points to `$DEVENV_DOTFILE/state`.

### [`DEVENV_RUNTIME`](#devenv_runtime)
<small class="added-in">added in `1.0`</small>

**Read-only.** Points to a temporary directory with a path that's unique to each
`$DEVENV_ROOT`, used for storing sockets and other runtime files.
Defaults to `$XDG_RUNTIME_DIR` and falls back to `$TMPDIR` and finally `/tmp`.

### [`DEVENV_PROFILE`](#devenv_profile)
<small class="added-in">added in `0.5`</small>

**Read-only.** Points to the Nix store path that has the final profile of
packages/scripts provided by devenv.
Useful for teaching other programs about `/bin`, `/etc`, `/var` folders.

### [`DEVENV_HOME`](#devenv_home)
<small class="added-in">added in `1.0`</small>

devenv's per-user data directory, at `~/.local/share/devenv` (respecting
`$XDG_DATA_HOME`).
Stores GC roots, the trust database, and other persistent per-user data.

### [`DEVENV_MAX_JOBS`](#devenv_max_jobs)
<small class="added-in">added in `1.11`</small>

Maximum number of Nix builds to run concurrently.
Mirrors the `-j` / `--max-jobs` flag.
Defaults to 1/4 of available CPU cores (minimum 1).

### [`DEVENV_CORES`](#devenv_cores)
<small class="added-in">added in `1.11`</small>

Number of CPU cores available to each build.
Mirrors the `-u` / `--cores` flag.
Defaults to available cores divided by `DEVENV_MAX_JOBS` (minimum 1).

### [`DEVENV_SHELL_TYPE`](#devenv_shell_type)
<small class="added-in">added in `2.1`</small>

Shell to use for interactive sessions: `bash`, `zsh`, `fish`, or `nu`.
Mirrors the `--shell` flag.

### [`DEVENV_TUI`](#devenv_tui)
<small class="added-in">added in `2.0`</small>

Enable (`true`) or disable (`false`) the interactive terminal interface.
Mirrors the `--tui` / `--no-tui` flags.
Enabled by default when the session is interactive.

### [`DEVENV_TRACE_TO`](#devenv_trace_to)
<small class="added-in">added in `2.1`</small>

Enable tracing to one or more destinations, comma-separated (e.g.
`pretty:stderr,json:file:/tmp/trace.json`).
Mirrors the `--trace-to` flag; run `devenv --help` for the full syntax.
`DEVENV_TRACE_DEFAULT_TO` sets a fallback used only when no tracing is otherwise
configured.

### [`DEVENV_INCLUDE_ENVRC`](#devenv_include_envrc)
<small class="added-in">added in `2.1.3`</small>

Generate an `.envrc` file when running `devenv init`.
Mirrors `devenv init --include-envrc`.

### [`DEVENV_NO_AI_AGENT`](#devenv_no_ai_agent)
<small class="added-in">added in `2.1`</small>

Set to any value to skip AI-agent auto-detection, forcing normal output and the
TUI even when running under a detected coding agent.

## Externally defined variables

devenv also reads the following externally defined environment variables.

### [`SHELL`](#shell)

Detects your default shell dialect and resolves the shell binary to launch for
`devenv shell` and hooks.

### [`HOME`](#home)

Locates your shell rc files and serves as the base for the XDG default paths.

### [`XDG_RUNTIME_DIR`](#xdg_runtime_dir)

Base directory for `$DEVENV_RUNTIME`. Falls back to `$TMPDIR`, then `/tmp`.

### [`XDG_DATA_HOME`](#xdg_data_home)

Base directory for `$DEVENV_HOME` (GC roots, trust database, cached keys).

### [`XDG_CONFIG_HOME`](#xdg_config_home)

Locates shell rc files and the Cachix CLI configuration.

### [`TMPDIR`](#tmpdir)

Base directory for build artifacts and a fallback for `$DEVENV_RUNTIME`.

### [`CI`](#ci)

When set, disables the interactive terminal interface by default.

### [`RUST_LOG`](#rust_log)

Sets the level and filter for devenv's own diagnostic logging (see also `--verbose`).

### [`NO_COLOR`](#no_color)

Disables colored output. `CLICOLOR` and `CLICOLOR_FORCE` are also honored.

### [`TERM`](#term)

Used to detect terminal color and Unicode capabilities (along with `COLORTERM`).

### [`HTTP_PROXY`](#http_proxy)

Standard proxy configuration for devenv's outgoing HTTP requests. `HTTPS_PROXY`,
`ALL_PROXY`, and `NO_PROXY` (and their lowercase forms) are honored too.

### [`CACHIX_AUTH_TOKEN`](#cachix_auth_token)

Authentication token for pulling from and pushing to Cachix binary caches.

### [`SECRETSPEC_PROVIDER`](#secretspec_provider)

Override the provider for the [secretspec integration](../integrations/secretspec.md).
Mirrored by the `--secretspec-provider` flag.

### [`SECRETSPEC_PROFILE`](#secretspec_profile)

Override the profile for the [secretspec integration](../integrations/secretspec.md).
Mirrored by the `--secretspec-profile` flag.
