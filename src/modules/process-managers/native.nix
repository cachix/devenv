{ pkgs, config, lib, ... }:
let
  cfg = config.process.managers.native;
  readyType = import ../lib/ready.nix { inherit lib; };
  processType = lib.types.submodule ({ config, ... }: {
    options = {
      listen = lib.mkOption {
        type = lib.types.listOf (lib.types.submodule {
          options = {
            name = lib.mkOption {
              type = lib.types.str;
              description = "Name of the socket (used in LISTEN_FDNAMES)";
            };

            kind = lib.mkOption {
              type = lib.types.enum [ "tcp" "unix_stream" ];
              default = "tcp";
              description = "Type of socket";
            };

            address = lib.mkOption {
              type = lib.types.nullOr lib.types.str;
              default = null;
              description = "TCP address (e.g., 127.0.0.1:8080) - required for tcp kind";
            };

            path = lib.mkOption {
              type = lib.types.nullOr lib.types.path;
              default = null;
              description = "Unix socket path - required for unix_stream kind";
            };

            backlog = lib.mkOption {
              type = lib.types.int;
              default = 128;
              description = "Socket listen backlog";
            };

            mode = lib.mkOption {
              type = lib.types.nullOr lib.types.int;
              default = null;
              description = "Unix socket file permissions (octal, e.g., 0o600)";
            };
          };
        });
        default = [ ];
        description = ''
          Socket activation configuration (systemd-compatible).

          The process will receive activated sockets via LISTEN_FDS/LISTEN_PID/LISTEN_FDNAMES
          environment variables, starting at file descriptor 3.
        '';
        example = lib.literalExpression ''
          [
            {
              name = "http";
              kind = "tcp";
              address = "127.0.0.1:8080";
            }
            {
              name = "api";
              kind = "unix_stream";
              path = "/tmp/api.sock";
              mode = 384; # 0o600
            }
          ]
        '';
      };

      watchdog = lib.mkOption {
        type = lib.types.nullOr (lib.types.submodule {
          options = {
            usec = lib.mkOption {
              type = lib.types.int;
              description = "Watchdog interval in microseconds";
            };

            require_ready = lib.mkOption {
              type = lib.types.bool;
              default = true;
              description = "Require READY=1 notification before enforcing watchdog";
            };
          };
        });
        default = null;
        description = ''
          Systemd watchdog configuration.

          The process should send WATCHDOG=1 via notify socket periodically.
        '';
        example = lib.literalExpression ''
          {
            usec = 30000000; # 30 seconds
            require_ready = true;
          }
        '';
      };

      ready = lib.mkOption {
        type = lib.types.nullOr readyType;
        default = null;
        description = "Readiness probe configuration.";
      };

      watch = lib.mkOption {
        type = lib.types.submodule {
          options = {
            paths = lib.mkOption {
              type = lib.types.listOf lib.types.path;
              default = [ ];
              description = "Paths to watch for changes";
            };
            extensions = lib.mkOption {
              type = lib.types.listOf lib.types.str;
              default = [ ];
              description = "File extensions to watch";
            };
            ignore = lib.mkOption {
              type = lib.types.listOf lib.types.str;
              default = [ ];
              description = "Glob patterns to ignore";
            };
          };
        };
        default = { };
        description = "File watching configuration";
      };
    };
  });
in
{
  options.process.managers.native = {
    enable = lib.mkEnableOption "native Rust process manager" // {
      internal = true;
    };

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.bashInteractive;
      defaultText = lib.literalExpression "pkgs.bashInteractive";
      description = "The shell package to use for running processes.";
      internal = true;
    };

    processConfig = lib.mkOption {
      type = lib.types.attrsOf processType;
      internal = true;
      description = "Per-process native manager configuration";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [ cfg.package ];

    # Wire up native-specific process configuration from process.process-compose for migration
    process.managers.native.processConfig = lib.mapAttrs
      (name: process:
        let
          pc = process.process-compose or { };
        in
        {
          listen = pc.listen or [ ];
          watchdog = pc.watchdog or null;
          ready = process.ready;
          inherit (process) watch;
        }
      )
      config.processes;

    # Export process configurations as JSON for the native manager
    process.nativeConfigJson = pkgs.writeText "process-config.json" (builtins.toJSON (
      lib.mapAttrs
        (name: process:
          let
            native = cfg.processConfig.${name};
          in
          removeAttrs process [ "process-compose" "ports" "notify" "ready" "restart" ] // {
            inherit name;
            inherit (native) watchdog;
            ready = native.ready;
            restart = process.restart;
            listen = map
              (spec: removeAttrs spec [ "path" ] // {
                path = if spec.path != null then toString spec.path else null;
              })
              native.listen;
            ports = lib.mapAttrs (_: portCfg: portCfg.value) process.ports;
            watch = native.watch // {
              paths = map toString native.watch.paths;
            };
          }
        )
        config.processes
    ));

    # The actual process manager command will be invoked from devenv.rs
    # We just need to provide the configuration via procfileScript
    process.manager.command = lib.mkDefault ''
      # Native process manager is invoked directly from devenv up
      # This script should not be reached
      echo "Native process manager should be invoked from devenv up" >&2
      exit 1
    '';
  };
}
