{ pkgs, lib, config, ... }:

with lib;

let
  cfg = config.services.redis;

  REDIS_UNIX_SOCKET = "${config.env.DEVENV_RUNTIME}/redis.sock";

  redisConfig = pkgs.writeText "redis.conf" ''
    port ${toString cfg.port}
    ${optionalString (cfg.bind != null) "bind ${cfg.bind}"}
    ${optionalString (cfg.port == 0) "unixsocket ${REDIS_UNIX_SOCKET}"}
    ${optionalString (cfg.port == 0) "unixsocketperm 700"}
    ${cfg.extraConfig}
  '';

  startScript = pkgs.writeShellScriptBin "start-redis" ''
    set -euo pipefail

    if [[ ! -d "$REDISDATA" ]]; then
      mkdir -p "$REDISDATA"
    fi

    exec ${cfg.package}/bin/redis-server ${redisConfig} --daemonize no --dir "$REDISDATA"
  '';

  tcpPing = "${cfg.package}/bin/redis-cli -p ${toString cfg.port} ping";
  unixSocketPing = "${cfg.package}/bin/redis-cli -s ${REDIS_UNIX_SOCKET} ping";
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
        If port 0 is specified Redis, will not listen on a TCP socket and a unix socket file will be found at $REDIS_UNIX_SOCKET.
      '';
    };

    extraConfig = mkOption {
      type = types.lines;
      default = "locale-collate C";
      description = "Additional text to be appended to `redis.conf`.";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ];

    env = {
      REDISDATA = config.env.DEVENV_STATE + "/redis";
      REDIS_UNIX_SOCKET = if cfg.port == 0 then REDIS_UNIX_SOCKET else null;
    };

    processes.redis = {
      exec = "${startScript}/bin/start-redis";

      process-compose = {
        readiness_probe = {
          exec.command = if cfg.port == 0 then unixSocketPing else tcpPing;
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
