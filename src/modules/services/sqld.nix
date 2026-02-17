{ pkgs
, lib
, config
, ...
}:

let
  cfg = config.services.sqld;
  qs = lib.escapeShellArgs;

  # Port allocation
  basePort = cfg.port;
  allocatedPort = config.processes.sqld.ports.main.value;
in
{
  options.services.sqld = {
    enable = lib.mkEnableOption "sqld";

    port = lib.mkOption {
      type = lib.types.port;
      default = 8080;
      description = "Port number to listen on.";
    };

    extraArgs = lib.mkOption {
      type = with lib.types; listOf str;
      default = [ ];
      description = "Add other sqld flags.";
    };
  };

  config = lib.mkIf cfg.enable {
    processes.sqld = {
      ports.main.allocate = basePort;
      exec = "exec ${pkgs.sqld}/bin/sqld --http-listen-addr 127.0.0.1:${toString allocatedPort} ${qs cfg.extraArgs}";

      ready = {
        http.get = {
          path = "/health";
          port = allocatedPort;
        };
        initial_delay = 2;
      };
    };
  };
}
