# Processes

Devenv provides built-in process management with supervision, socket activation, file watching, and dependency management.

## Basic Example

```nix title="devenv.nix"
{ pkgs, ... }:

{
  processes = {
    silly-example.exec = "while true; do echo hello && sleep 1; done";
    ping.exec = "ping localhost";
    server = {
      exec = "python -m http.server";
      cwd = "./public";
    };
  };
}
```

To start the processes, run:

```shell-session
$ devenv up
```

## Restart Policies

Control how processes restart when they exit:

- `on_failure` (default) - restart only on non-zero exit
- `always` - restart on any exit
- `never` - never restart

```nix title="devenv.nix"
{
  processes.worker = {
    exec = "worker --queue jobs";
    restart = "always";
    max_restarts = 10;  # null for unlimited
  };
}
```

## Process Types

- `foreground` (default) - long-running process that restarts based on policy
- `oneshot` - run-to-completion task that executes once

```nix title="devenv.nix"
{
  processes = {
    migrate = {
      exec = "diesel migration run";
      type = "oneshot";
    };

    server = {
      exec = "cargo run --release";
      type = "foreground";
      after = [ "devenv:processes:migrate@complete" ];
    };
  };
}
```

## Dependencies

Processes can depend on other processes using `after` and `before`:

```nix title="devenv.nix"
{
  processes = {
    database.exec = "postgres";

    api = {
      exec = "myapi";
      after = [ "devenv:processes:database" ];  # wait for database to be ready
    };
  };
}
```

Use `@complete` suffix to wait for oneshot processes to finish, or `@ready` (default) for readiness.

## File Watching

Automatically restart processes when files change:

```nix title="devenv.nix"
{
  processes.backend = {
    exec = "cargo run";
    watch = {
      paths = [ ./src ];
      extensions = [ "rs" "toml" ];
      ignore = [ "target" "*.log" ];
    };
  };
}
```

## Socket Activation

Socket activation allows the process manager to bind sockets before starting your process. This enables zero-downtime restarts and lazy process startup.

```nix title="devenv.nix"
{
  processes.api = {
    exec = "myserver";
    listen = [
      {
        name = "http";
        kind = "tcp";
        address = "127.0.0.1:8080";
      }
      {
        name = "admin";
        kind = "unix_stream";
        path = "$DEVENV_STATE/admin.sock";
      }
    ];
  };
}
```

Your process receives these environment variables:

- `LISTEN_FDS` - number of passed file descriptors
- `LISTEN_PID` - PID that should accept the sockets
- `LISTEN_FDNAMES` - colon-separated socket names

File descriptors start at 3 (after stdin, stdout, stderr). This is compatible with systemd socket activation.

## Notify Protocol

Enable systemd-style readiness notification. Your process should send `READY=1` to the socket path in `$NOTIFY_SOCKET` when ready.

```nix title="devenv.nix"
{
  processes.database = {
    exec = "postgres";
    notify.enable = true;
  };

  processes.api = {
    exec = "myapi";
    after = [ "devenv:processes:database" ];  # waits for READY=1
  };
}
```

## Using Pre-built Services

Devenv provides many pre-configured services with proper process management. See the [Services documentation](services/index.md) for available services like:

- [PostgreSQL](services/postgres.md)
- [Redis](services/redis.md)
- [MySQL](services/mysql.md)
- [MongoDB](services/mongodb.md)
- [Elasticsearch](services/elasticsearch.md)

These services come with sensible defaults, health checks, and proper initialization scripts.

## Git Integration

Processes can reference the git repository root path using `${config.git.root}`, useful in monorepo environments:

```nix title="devenv.nix"
{ config, ... }:

{
  processes.frontend = {
    exec = "npm run dev";
    cwd = "${config.git.root}/frontend";
  };

  processes.backend = {
    exec = "cargo run";
    cwd = "${config.git.root}/backend";
  };
}
```

Processes are automatically available as tasks, allowing you to define pre and post hooks. See the [Processes as tasks](tasks.md#processes-as-tasks) section for details.

## Automatic port allocation

!!! tip "New in devenv 2.0"

Devenv can automatically allocate free ports for your processes, preventing conflicts when a port is already in use or when running multiple devenv projects simultaneously.

Define ports using `ports.<name>.allocate` with a base port number. Devenv will find a free port starting from that base, incrementing until one is available:

```nix title="devenv.nix"
{ config, ... }:

{
  processes.server = {
    ports.http.allocate = 8080;
    ports.admin.allocate = 9000;
    exec = ''
      echo "HTTP server on port ${toString config.processes.server.ports.http.value}"
      echo "Admin panel on port ${toString config.processes.server.ports.admin.value}"
      python -m http.server ${toString config.processes.server.ports.http.value}
    '';
  };
}
```

The resolved port is available via `config.processes.<name>.ports.<port>.value`. If port 8080 is already in use, devenv will automatically try 8081, 8082, and so on until it finds an available port.

Devenv holds the allocated ports during configuration evaluation to prevent race conditions, then releases them just before starting the processes so your application can bind to them.

This is particularly useful for:

- **Running multiple projects**: Each project gets its own ports without manual coordination
- **CI environments**: Tests can run in parallel without port conflicts
- **Shared development machines**: Multiple developers can run the same project simultaneously

### Strict port mode

If you want devenv to fail when a port is already in use instead of automatically finding the next available port, use the `--strict-ports` flag:

```shell-session
$ devenv up --strict-ports
```

This is useful when you need deterministic port assignments and want to be notified of conflicts rather than having them silently resolved. When a port conflict is detected in strict mode, devenv will show an error message including which process is currently using the port.

## Alternative Process Managers

By default, devenv uses its native process manager. You can switch to alternative implementations:

- [process-compose](supported-process-managers/process-compose.md) - Feature-rich external process manager with TUI
- [overmind](supported-process-managers/overmind.md) - Procfile-based with tmux integration
- [honcho](supported-process-managers/honcho.md) - Python Foreman port
- [hivemind](supported-process-managers/hivemind.md) - Simple Procfile manager
- [mprocs](supported-process-managers/mprocs.md) - TUI process manager

To switch:

```nix title="devenv.nix"
{
  process.manager.implementation = "process-compose";
}
```
