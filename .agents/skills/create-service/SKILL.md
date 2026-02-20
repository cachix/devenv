---
name: create-service
description: This skill should be used when the user asks to "create a service module", "add a new service", "write a service for", "implement a service module", or wants to add a new service to src/modules/services/. Provides the patterns and conventions for devenv service modules.
argument-hint: [service-name]
---

# Create a devenv Service Module

This skill guides the creation of new service modules under `src/modules/services/`.

## Process

1. Research the service: default port, package name in nixpkgs, config file format, socket activation support, systemd notify/watchdog support
2. Read existing modules in `src/modules/services/` for reference (e.g., `memcached.nix` for simple, `redis.nix` for medium, `minio.nix` for complex)
3. Create `src/modules/services/<name>.nix` following the patterns below
4. Register the module in `src/modules/services.nix`
5. Add a test under `tests/` or `examples/`

## Unix Sockets Preferred

When a service supports unix sockets, prefer them over TCP ports as the default communication method. Unix sockets are faster, avoid port conflicts, and are more secure for local-only services. See `redis.nix` for the pattern: use `DEVENV_RUNTIME` for the socket path, expose `$<NAME>_UNIX_SOCKET` env var, and fall back to TCP only when the user explicitly configures a port.

## Module Structure

Every service module follows this skeleton:

```nix
{ pkgs, lib, config, ... }:

let
  cfg = config.services.<name>;
  types = lib.types;

  # Port allocation
  basePort = cfg.port;
  allocatedPort = config.processes.<name>.ports.main.value;
in
{
  imports = [
    # Backward compat: only add if migrating from old top-level options
    # (lib.mkRenamedOptionModule [ "<name>" "enable" ] [ "services" "<name>" "enable" ])
  ];

  options.services.<name> = {
    enable = lib.mkEnableOption "<human-readable description>";

    package = lib.mkOption {
      type = types.package;
      description = "Which package of <name> to use";
      default = pkgs.<name>;
      defaultText = lib.literalExpression "pkgs.<name>";
    };

    bind = lib.mkOption {
      type = types.nullOr types.str;
      default = "127.0.0.1";
      description = ''
        The IP interface to bind to.
        `null` means "all interfaces".
      '';
    };

    port = lib.mkOption {
      type = types.port;
      default = <default-port>;
      description = "The TCP port to accept connections.";
    };

    # Add service-specific options here
  };

  config = lib.mkIf cfg.enable {
    packages = [ cfg.package ];

    env.<NAME>_PORT = allocatedPort;

    processes.<name> = {
      ports.main.allocate = basePort;
      exec = "exec ${cfg.package}/bin/<binary> <args>";

      # Only needed for non-TCP health checks (see Readiness Probes below)
      # ready = { ... };
    };
  };
}
```

## Key Conventions

### Port Allocation

Always use the dynamic port allocation system, never hardcode ports:

```nix
basePort = cfg.port;
allocatedPort = config.processes.<name>.ports.main.value;
# ...
processes.<name>.ports.main.allocate = basePort;
```

For multiple ports, use named ports:

```nix
allocatedHttpPort = config.processes.<name>.ports.http.value;
allocatedGrpcPort = config.processes.<name>.ports.grpc.value;
# ...
processes.<name>.ports.http.allocate = baseHttpPort;
processes.<name>.ports.grpc.allocate = baseGrpcPort;
```

### Data and Runtime Directories

Use devenv standard paths:

```nix
env.<NAME>_DATA = config.env.DEVENV_STATE + "/<name>";      # persistent data
env.<NAME>_RUNTIME = config.env.DEVENV_RUNTIME + "/<name>";  # runtime/socket files
```

### Readiness Probes

The native process manager automatically creates a TCP ready probe for the first allocated port or listen socket. For most TCP services, **no explicit `ready` block is needed** — just allocating a port is sufficient.

Only add an explicit `ready` block when you need a custom health check:

- **HTTP health endpoint**: `ready.http.get = { host = cfg.bind; port = allocatedPort; path = "/health"; };`
- **CLI tools**: `ready.exec = "${cfg.package}/bin/<client> ping";` (e.g., `redis-cli ping`, `pg_isready`)
- **Multi-step checks**: `ready.exec` with a script that verifies initialization beyond port availability

### Socket Activation

When a service supports systemd socket activation (`LISTEN_FDS`/`LISTEN_PID`), prefer it over port allocation. Socket activation eliminates race conditions (the socket is listening before the process starts) and enables zero-downtime restarts.

```nix
processes.<name> = {
  exec = "exec ${cfg.package}/bin/<binary> <args>";

  listen = [
    # TCP socket
    { name = "http"; kind = "tcp"; address = "${cfg.bind}:${toString allocatedPort}"; }
    # Unix socket
    { name = "main"; kind = "unix_stream"; path = "${config.env.DEVENV_RUNTIME}/<name>/<name>.sock"; mode = 384; } # 0o600
  ];
};
```

The supervisor auto-probes TCP listen sockets for readiness (higher priority than allocated ports). Check if the service supports socket activation by looking for `LISTEN_FDS` or `SD_LISTEN_FDS_START` in its documentation.

### Systemd Notify and Watchdog

When a service supports the systemd notify protocol (`sd_notify(3)`), use it for precise readiness signaling instead of TCP probing. The process receives `NOTIFY_SOCKET` and sends `READY=1` when fully initialized.

```nix
processes.<name> = {
  exec = "exec ${cfg.package}/bin/<binary> <args>";
  ready.notify = true;
};
```

For long-running services that support watchdog, enable it so the supervisor can detect hangs and restart automatically. The process must send periodic `WATCHDOG=1` pings.

```nix
processes.<name> = {
  exec = "exec ${cfg.package}/bin/<binary> <args>";
  ready.notify = true;
  watchdog = {
    usec = 30000000; # 30 seconds
    require_ready = true; # only enforce after READY=1
  };
};
```

Check the service's documentation for `Type=notify`, `WatchdogSec=`, or `sd_notify` support.

### Setup and Cleanup Tasks

When a service needs initialization (e.g., creating data directories, initializing databases) or cleanup, use tasks instead of wrapping the process exec in a startup script. Tasks are cached, run in the correct order via the DAG, and are visible in the TUI.

```nix
tasks."devenv:<name>:setup" = {
  exec = ''
    mkdir -p "$<NAME>_DATA"
    # any other initialization
  '';
  before = [ "devenv:processes:<name>" ];
};
```

Use `before` to ensure the task runs before the process starts. The process `exec` should remain a simple `exec` into the service binary — no shell wrapper needed.

### Configuration Files

Generate config files with `pkgs.writeText` or `pkgs.formats`:

```nix
# Plain text config
configFile = pkgs.writeText "<name>.conf" ''
  port ${toString allocatedPort}
  ${cfg.extraConfig}
'';

# Structured config (INI, JSON, YAML, etc.)
format = pkgs.formats.ini { };
configFile = format.generate "<name>.conf" cfg.settings;
```

### Registration

After creating the module, register it in `src/modules/services.nix`.
