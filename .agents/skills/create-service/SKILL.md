---
name: create-service
description: This skill should be used when the user asks to "create a service module", "add a new service", "write a service for", "implement a service module", or wants to add a new service to src/modules/services/. Provides the patterns and conventions for devenv service modules.
argument-hint: [service-name]
---

# Create a devenv Service Module

This skill guides the creation of new service modules under `src/modules/services/`.

## Process

1. Research the service: default port, package name in nixpkgs, config file format, health check method
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

      ready = {
        exec = "<health-check-command>";
        initial_delay = 2;
        probe_timeout = 4;
        failure_threshold = 5;
      };
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

Choose the appropriate health check method:

- **TCP services**: `echo | ${pkgs.netcat}/bin/nc <host> <port>`
- **HTTP services**: `${pkgs.curl}/bin/curl -f http://<host>:<port>/health`
- **CLI tools**: Use the service's own client (e.g., `redis-cli ping`, `pg_isready`)

### Start Scripts

For services needing initialization, use `pkgs.writeShellScriptBin`:

```nix
startScript = pkgs.writeShellScriptBin "start-<name>" ''
  set -euo pipefail
  mkdir -p "$<NAME>_DATA"
  exec ${cfg.package}/bin/<binary> <args>
'';
# ...
processes.<name>.exec = "${startScript}/bin/start-<name>";
```

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
