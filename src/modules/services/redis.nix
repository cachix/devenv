{ pkgs, lib, config, ... }:

with lib;

let
  cfg = config.services.redis;

  redisConfig = pkgs.writeText "redis.conf" ''
    port ${toString cfg.port}
    ${optionalString (cfg.bind != null) "bind ${cfg.bind}"}
    ${cfg.extraConfig}
  '';

  startScript = pkgs.writeShellScriptBin "start-redis" ''
    set -euo pipefail

    if [[ ! -d "$REDISDATA" ]]; then
      mkdir -p "$REDISDATA"
    fi

    exec ${cfg.package}/bin/redis-server ${redisConfig} --dir "$REDISDATA"
  '';
in
{
  imports = [
    (lib.mkRenamedOptionModule [ "redis" "enable" ] [ "services" "redis" "enable" ])
  ];

  options.services.redis = {
    enable = mkEnableOption "Redis process and expose utilities";

    package = mkOption {
      type = types.package;
      description = "Which package of Redis to use";
      default = pkgs.redis;
      defaultText = lib.literalExpression "pkgs.redis";
    };

    bind = mkOption {
      type = types.nullOr types.str;
      default = "127.0.0.1";
      description = ''
        The IP interface to bind to.
        `null` means "all interfaces".
      '';
      example = "127.0.0.1";
    };

    port = mkOption {
      type = types.port;
      default = 6379;
      description = ''
        The TCP port to accept connections.
        If port 0 is specified Redis, will not listen on a TCP socket.
      '';
    };

    extraConfig = mkOption {
      type = types.lines;
      default = "";
      description = "Additional text to be appended to `redis.conf`.";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ];

    env.REDISDATA = config.env.DEVENV_STATE + "/redis";

    processes.redis = {
      exec = "${startScript}/bin/start-redis";

      process-compose = {
        readiness_probe = {
          exec.command = "${cfg.package}/bin/redis-cli -p ${toString cfg.port} ping";
          initial_delay_seconds = 2;
          period_seconds = 10;
          timeout_seconds = 4;
          success_threshold = 1;
          failure_threshold = 5;
        };

        # https://github.com/F1bonacc1/process-compose#-auto-restart-if-not-healthy
        availability.restart = "on_failure";
      };
    };
  };
}
