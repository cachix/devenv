{ pkgs, lib, config, ... }:

let
  cfg = config.memcached;
  types = lib.types;
in
{
  options.memcached = {
    enable = lib.mkEnableOption "Add memcached process.";

    package = lib.mkOption {
      type = types.package;
      description = "Which package of memcached to use";
      default = pkgs.memcached;
      defaultText = "pkgs.memcached";
    };

    bind = lib.mkOption {
      type = types.nullOr types.str;
      default = "127.0.0.1";
      description = lib.mdDoc ''
        The IP interface to bind to.
        `null` means "all interfaces".
      '';
      example = "127.0.0.1";
    };

    port = lib.mkOption {
      type = types.port;
      default = 11211;
      description = ''
        The TCP port to accept connections.
        If port 0 is specified Redis will not listen on a TCP socket.
      '';
    };

    startArgs = lib.mkOption {
      type = types.listOf types.lines;
      default = [ ];
      example = [ "--memory-limit=100M" ];
      description = ''
        Additional arguments passed to `memcached` during startup.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    processes.memcached.exec = "${cfg.package}/bin/memcached --port=${toString cfg.port} --listen=${cfg.bind} ${lib.concatStringsSep " " cfg.startArgs}";
  };
}
