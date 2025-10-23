{ config, lib, pkgs, ... }:

with lib;

let
  cfg = config.services.nats;

  # Generate NATS config file from settings (only if settings are provided)
  configFile = pkgs.writeText "nats.conf" (builtins.toJSON cfg.settings);

  # Build command-line arguments
  buildArgs = concatStringsSep " " (
    [ "-a ${cfg.host}" ]
    ++ [ "-p ${toString cfg.port}" ]
    ++ optional (cfg.serverName != "") "-n ${cfg.serverName}"
    ++ optional (cfg.clientAdvertise != "") "--client_advertise ${cfg.clientAdvertise}"
    ++ optional cfg.jetstream.enable "-js"
    ++ optional cfg.monitoring.enable "-m ${toString cfg.monitoring.port}"
    ++ optional (cfg.logFile != "") "-l ${cfg.logFile}"
    ++ optional cfg.debug "-D"
    ++ optional cfg.trace "-V"
    ++ optional (cfg.settings != { }) "-c ${configFile}"
  );
in
{
  options.services.nats = {
    enable = mkOption {
      type = types.bool;
      default = false;
      description = ''
        Whether to enable the NATS messaging server.

        NATS is a simple, secure and high performance messaging
        system for cloud native applications, IoT messaging,
        and microservices architectures.
      '';
    };

    package = mkOption {
      type = types.package;
      default = pkgs.nats-server;
      defaultText = literalExpression "pkgs.nats-server";
      description = ''
        Which NATS server package to use.
      '';
    };

    host = mkOption {
      type = types.str;
      default = "127.0.0.1";
      example = "0.0.0.0";
      description = ''
        Network host to listen on for client connections.
        Set to "0.0.0.0" to listen on all interfaces.
        Default is localhost for security.
      '';
    };

    port = mkOption {
      type = types.port;
      default = 4222;
      description = ''
        Port to listen on for client connections.
        Default NATS client port is 4222.
      '';
    };

    serverName = mkOption {
      type = types.str;
      default = "";
      example = "nats-dev-1";
      description = ''
        Server name for identification in clusters.
        If empty, NATS will auto-generate a unique name.
      '';
    };

    clientAdvertise = mkOption {
      type = types.str;
      default = "";
      example = "localhost:4222";
      description = ''
        Client URL to advertise to other servers in a cluster.
        Useful when running behind NAT or in containers.
      '';
    };

    jetstream = {
      enable = mkEnableOption "JetStream persistence layer for streaming and queues";

      maxMemory = mkOption {
        type = types.str;
        default = "1G";
        example = "512M";
        description = ''
          Maximum memory for in-memory streams.
          Use suffixes: K, M, G, T for sizes.
        '';
      };

      maxFileStore = mkOption {
        type = types.str;
        default = "10G";
        example = "100G";
        description = ''
          Maximum disk space for file-based streams.
          Use suffixes: K, M, G, T for sizes.
        '';
      };
    };

    monitoring = {
      enable = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Enable HTTP monitoring endpoint.
          Provides /healthz, /varz, /connz, and other monitoring endpoints.
          Highly recommended for production deployments.
        '';
      };

      port = mkOption {
        type = types.port;
        default = 8222;
        description = ''
          Port for HTTP monitoring endpoint.
          Access monitoring at http://host:port/varz
        '';
      };
    };

    logFile = mkOption {
      type = types.str;
      default = "";
      example = "/var/log/nats-server.log";
      description = ''
        Path to log file. If empty, logs to stdout.
        Stdout is recommended for devenv as logs are captured by process manager.
      '';
    };

    debug = mkOption {
      type = types.bool;
      default = false;
      description = ''
        Enable debug logging for troubleshooting.
      '';
    };

    trace = mkOption {
      type = types.bool;
      default = false;
      description = ''
        Enable protocol tracing for deep debugging.
        Warning: Very verbose output.
      '';
    };

    authorization = {
      enable = mkEnableOption "authorization for client connections";

      user = mkOption {
        type = types.str;
        default = "";
        example = "nats-user";
        description = ''
          Username required for client connections.
          Only used if authorization is enabled.
        '';
      };

      password = mkOption {
        type = types.str;
        default = "";
        example = "nats-pass";
        description = ''
          Password required for client connections.
          Only used if authorization is enabled.
          Warning: This will be visible in the Nix store.
        '';
      };

      token = mkOption {
        type = types.str;
        default = "";
        example = "my-secret-token";
        description = ''
          Token required for client connections.
          Alternative to user/password authentication.
          Warning: This will be visible in the Nix store.
        '';
      };
    };

    settings = mkOption {
      type = types.attrs;
      default = { };
      example = literalExpression ''
        {
          tls = {
            cert_file = "/path/to/cert.pem";
            key_file = "/path/to/key.pem";
            verify = true;
          };
          cluster = {
            name = "my-cluster";
            listen = "0.0.0.0:6222";
            routes = [
              "nats://node1:6222"
              "nats://node2:6222"
            ];
          };
        }
      '';
      description = ''
        Additional NATS server configuration as a Nix attribute set.
        This will be converted to NATS config file format.

        Use this for advanced features like:
        - TLS/SSL configuration
        - Clustering with routes
        - MQTT gateway
        - WebSocket support
        - Custom authorization

        See https://docs.nats.io/running-a-nats-service/configuration
      '';
    };
  };

  config = mkIf cfg.enable {
    packages = [ cfg.package ];

    # Merge basic options into settings for config file generation
    # Note: User-provided cfg.settings will be automatically merged by the module system
    services.nats.settings = mkMerge [
      # JetStream settings (if enabled)
      (mkIf cfg.jetstream.enable {
        jetstream = {
          store_dir = config.env.DEVENV_STATE + "/nats/jetstream";
          max_memory_store = cfg.jetstream.maxMemory;
          max_file_store = cfg.jetstream.maxFileStore;
        };
      })

      # Authorization settings (if enabled)
      (mkIf cfg.authorization.enable (
        {
          authorization = { } //
            (optionalAttrs (cfg.authorization.user != "") { user = cfg.authorization.user; }) //
            (optionalAttrs (cfg.authorization.password != "") { password = cfg.authorization.password; }) //
            (optionalAttrs (cfg.authorization.token != "") { token = cfg.authorization.token; });
        }
      ))
    ];

    env.NATS_DATA_DIR = config.env.DEVENV_STATE + "/nats";

    # Create necessary directories
    enterShell = ''
      # Create NATS data directory
      mkdir -p ${config.env.DEVENV_STATE}/nats

      # Create JetStream directory if enabled
      ${optionalString cfg.jetstream.enable ''
        mkdir -p ${config.env.DEVENV_STATE}/nats/jetstream
      ''}
    '';

    processes.nats = {
      exec = "${cfg.package}/bin/nats-server ${buildArgs}";

      process-compose = {
        readiness_probe = {
          # Use HTTP healthz endpoint if monitoring is enabled, otherwise TCP check
          exec.command =
            if cfg.monitoring.enable then
              "${pkgs.curl}/bin/curl -f http://${cfg.host}:${toString cfg.monitoring.port}/healthz"
            else
              "${pkgs.netcat}/bin/nc -z ${cfg.host} ${toString cfg.port}";
          initial_delay_seconds = 2;
          period_seconds = 5;
          timeout_seconds = 3;
          success_threshold = 1;
          failure_threshold = 5;
        };

        # https://github.com/F1bonacc1/process-compose#-auto-restart-if-not-healthy
        availability.restart = "on_failure";
      };
    };
  };
}
