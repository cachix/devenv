{ lib }:

let
  types = lib.types;
in
types.submodule {
  options = {
    exec = lib.mkOption {
      type = types.nullOr types.str;
      default = null;
      description = "Shell command to execute. Exit 0 = ready.";
      example = "pg_isready -d template1";
    };

    http = lib.mkOption {
      type = types.submodule {
        options.get = lib.mkOption {
          type = types.nullOr (types.submodule {
            options = {
              host = lib.mkOption {
                type = types.str;
                default = "127.0.0.1";
                description = "Host to connect to.";
              };
              port = lib.mkOption {
                type = types.port;
                description = "Port to connect to.";
              };
              path = lib.mkOption {
                type = types.str;
                default = "/";
                description = "HTTP path to request.";
              };
              scheme = lib.mkOption {
                type = types.str;
                default = "http";
                description = "URL scheme (http or https).";
              };
            };
          });
          default = null;
          description = "HTTP GET readiness check.";
        };
      };
      default = { };
      description = "HTTP readiness probe configuration.";
    };

    notify = lib.mkOption {
      type = types.bool;
      default = false;
      description = ''
        Enable systemd notify protocol for readiness signaling.
        The process must send READY=1 to the NOTIFY_SOCKET.
      '';
    };

    initial_delay = lib.mkOption {
      type = types.int;
      default = 0;
      description = "Seconds to wait before first probe.";
    };

    period = lib.mkOption {
      type = types.int;
      default = 10;
      description = "Seconds between probes.";
    };

    probe_timeout = lib.mkOption {
      type = types.int;
      default = 1;
      description = "Seconds before a single probe times out.";
    };

    timeout = lib.mkOption {
      type = types.nullOr types.ints.unsigned;
      default = null;
      description = "Overall deadline in seconds for the process to become ready. null = no deadline.";
    };

    success_threshold = lib.mkOption {
      type = types.int;
      default = 1;
      description = "Consecutive successes needed to be considered ready.";
    };

    failure_threshold = lib.mkOption {
      type = types.int;
      default = 3;
      description = "Consecutive failures before marking unhealthy.";
    };
  };
}
