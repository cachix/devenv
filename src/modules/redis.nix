{ pkgs, lib, config, ... }:

with lib;

let
  cfg = config.redis;

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
  options.redis = {
    enable = mkEnableOption "Add redis process and expose utilities.";

    package = mkOption {
      type = types.package;
      description = "Which package of redis to use";
      default = pkgs.redis;
      defaultText = "pkgs.redis";
    };

    bind = mkOption {
      type = types.nullOr types.str;
      default = "127.0.0.1";
      description = mdDoc ''
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
        If port 0 is specified Redis will not listen on a TCP socket.
      '';
    };

    extraConfig = mkOption {
      type = types.lines;
      default = "";
      description = "Additional text to be appended to <filename>redis.conf</filename>.";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ];

    env.REDISDATA = config.env.DEVENV_STATE + "/redis";

    processes.redis.exec = "${startScript}/bin/start-redis";
  };
}
