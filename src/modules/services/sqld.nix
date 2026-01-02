{ pkgs
, lib
, config
, ...
}:

let
  cfg = config.services.sqld;
  qs = lib.escapeShellArgs;
in
{
  options.services.sqld = {
    enable = lib.mkEnableOption "sqld";

    port = lib.mkOption {
      type = lib.types.int;
      default = 8080;
      description = "Port number to listen on";
    };

    extraArgs = lib.mkOption {
      type = with lib.types; listOf str;
      default = [ ];
      description = "Add other sqld flags.";
    };
  };

  config = lib.mkIf cfg.enable {
    processes.sqld = {
      exec = "${pkgs.sqld}/bin/sqld --http-listen-addr 127.0.0.1:${toString cfg.port} ${qs cfg.extraArgs}";

      process-compose = {
        readiness_probe = {
          initial_delay_seconds = 2;
          http_get = {
            path = "/health";
            port = cfg.port;
          };
        };
      };
    };
  };
}
