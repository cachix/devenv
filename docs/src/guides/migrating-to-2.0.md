# Migrating to devenv 2.0

This guide covers the breaking changes in devenv 2.0 and how to update your project.

## Native process manager is the default

devenv 2.0 replaces process-compose with a built-in Rust process manager. If your processes work without process-compose-specific configuration, no changes are needed --- the native manager picks up `processes.*` definitions as before.

If you depend on process-compose features or want to keep using it during the transition:

```nix title="devenv.nix"
{
  process.manager.implementation = "process-compose";
}
```

The native manager supports port allocation, readiness probes, socket activation, file watching, dependency ordering, watchdog heartbeats, and Linux capabilities. See the [processes documentation](../processes.md) for details.

If there's something process-compose does that the native manager doesn't yet cover, please [let us know](https://github.com/cachix/devenv/issues).

### Migrating process-compose options

If you used `processes.<name>.process-compose` attributes, here's how to translate them to native equivalents.

#### Dependencies

process-compose uses `depends_on` with conditions. The native manager uses `after` with lifecycle suffixes:

```nix title="Before"
{
  processes.api.process-compose = {
    depends_on.postgres.condition = "process_healthy";
    depends_on.migrations.condition = "process_completed_successfully";
    depends_on.cleanup.condition = "process_completed";
  };
}
```

```nix title="After"
{
  processes.api.after = [
    "devenv:processes:postgres"                # waits for readiness probe (= process_healthy)
    "devenv:processes:migrations"              # waits for successful completion
    "devenv:processes:cleanup@complete"         # waits for exit regardless of success
  ];
}
```

| process-compose condition | Native equivalent |
|---|---|
| `process_healthy` | `"devenv:processes:X"` (requires a `ready` probe on X) |
| `process_completed_successfully` | `"devenv:processes:X"` |
| `process_completed` | `"devenv:processes:X@complete"` |
| `process_started` | No exact equivalent --- use `after` with a lightweight ready probe |

#### Restart policy

```nix title="Before"
{
  processes.api.process-compose = {
    availability = {
      restart = "on_failure";
      backoff_seconds = 2;
      max_restarts = 5;
    };
  };
}
```

```nix title="After"
{
  processes.api.restart = {
    on = "on_failure";  # "never" | "always" | "on_failure"
    max = 5;
    window = null;      # optional: sliding window in seconds for rate limiting
  };
}
```

Note: `backoff_seconds` has no native equivalent. The native manager restarts immediately.

#### Environment variables and working directory

```nix title="Before"
{
  processes.api.process-compose = {
    environment = [ "NODE_ENV=production" "PORT=3000" ];
    working_dir = "/app";
  };
}
```

```nix title="After"
{
  processes.api = {
    env = {
      NODE_ENV = "production";
      PORT = "3000";
    };
    cwd = "/app";
  };
}
```

#### Readiness probes

The `ready` option works with both managers, so if you already use it, no changes are needed. If you used `process-compose.readiness_probe` directly:

```nix title="Before"
{
  processes.api.process-compose = {
    readiness_probe = {
      exec.command = "curl -f http://localhost:8080/health";
      period_seconds = 5;
      failure_threshold = 3;
    };
  };
}
```

```nix title="After"
{
  processes.api.ready = {
    exec = "curl -f http://localhost:8080/health";
    period = 5;
    failure_threshold = 3;
  };
}
```

The native manager also supports HTTP probes and sd_notify:

```nix title="Native-only probe types"
{
  # HTTP probe
  processes.api.ready.http.get = { port = 8080; path = "/health"; };

  # sd_notify: process sends READY=1
  processes.app.ready.notify = true;
}
```

#### Liveness probes

process-compose supports `liveness_probe` separately from `readiness_probe`. The native manager has no liveness probe --- use `watchdog` as an alternative for long-running health monitoring:

```nix title="Before"
{
  processes.api.process-compose = {
    liveness_probe = {
      exec.command = "check-alive";
      period_seconds = 30;
    };
  };
}
```

```nix title="After"
{
  processes.api = {
    ready.notify = true;
    watchdog = {
      usec = 30000000;    # 30 seconds in microseconds
      require_ready = true;
    };
  };
}
```

The watchdog requires the process to send periodic `WATCHDOG=1` heartbeats via `NOTIFY_SOCKET`. If your process doesn't support sd_notify, wrap it:

```bash
# In your exec script:
while true; do systemd-notify WATCHDOG=1; sleep 10; done &
exec myapp
```

#### Shutdown signal

```nix title="Before"
{
  processes.postgres.process-compose = {
    shutdown.signal = 2;  # SIGINT
  };
}
```

The native manager sends SIGTERM. If your process needs a different signal, wrap it:

```nix title="After"
{
  processes.postgres.exec = ''
    trap 'kill -INT "$PID"' TERM
    postgres -D "$PGDATA" &
    PID=$!
    wait "$PID"
  '';
}
```

#### Elevated processes

```nix title="Before"
{
  processes.server.process-compose = {
    is_elevated = true;
  };
}
```

For specific privilege needs, use Linux capabilities instead:

```nix title="After"
{
  processes.server.linux.capabilities = [ "net_bind_service" ];
}
```

## git-hooks input is now optional

The `git-hooks` input is no longer included by default. If you use `git-hooks.hooks` in your `devenv.nix`, add the input explicitly:

```yaml title="devenv.yaml"
inputs:
  git-hooks:
    url: github:cachix/git-hooks.nix
```

If you don't use git-hooks, no changes are needed.

## `devenv build` returns JSON

`devenv build` now outputs JSON instead of plain store paths:

```shell-session
$ devenv build languages.rust.package
{
  "languages.rust.package": "/nix/store/...-rust-1.83.0"
}
```

Update any scripts that parse the output. For example, if you previously did:

```bash
store_path=$(devenv build languages.rust.package)
```

Use `jq` to extract the value:

```bash
store_path=$(devenv build languages.rust.package | jq -r '.["languages.rust.package"]')
```

## `devenv container` subcommand cleanup

`devenv container --copy <name>` has been removed. Use the subcommand form instead:

```shell-session
$ devenv container copy <name>
```
