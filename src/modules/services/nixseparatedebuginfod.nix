{ pkgs, lib, config, ... }:

let
  cfg = config.services.nixseparatedebuginfod;
  listen_address = "${cfg.host}:${toString cfg.port}";
in
{
  options.services.nixseparatedebuginfod = {
    enable = lib.mkEnableOption "nixseparatedebuginfod";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.nixseparatedebuginfod;
      description = "nixseparatedebuginfod package to use.";
    };

    host = lib.mkOption {
      type = lib.types.str;
      default = "127.0.0.1";
      description = "IP address for nixseparatedebuginfod to listen on.";
    };

    port = lib.mkOption {
      type = lib.types.port;
      default = 1949;
      description = "Port for nixseparatedebuginfod to listen on.";
    };
  };

  config = lib.mkIf cfg.enable {
    processes.nixseparatedebuginfod.exec = ''
      exec ${lib.getExe cfg.package} -l ${listen_address}
    '';

    enterShell = ''
      export DEBUGINFOD_URLS="http://${listen_address}''${DEBUGINFOD_URLS:+ $DEBUGINFOD_URLS}"
    '';
  };
}
