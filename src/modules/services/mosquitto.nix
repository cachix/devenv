{ pkgs, lib, config, ... }:

let
  cfg = config.services.mosquitto;
  types = lib.types;

  basePort = cfg.port;
  allocatedPort = config.processes.mosquitto.ports.main.value;

  configFile = pkgs.writeText "mosquitto.conf" ''
    allow_anonymous true
    listener ${toString allocatedPort}${lib.optionalString (cfg.bind != null) " ${cfg.bind}"}
    persistence false
    log_dest stderr
    ${cfg.extraConfig}
  '';
in
{
  imports = [
    (lib.mkRenamedOptionModule [ "mosquitto" "enable" ] [ "services" "mosquitto" "enable" ])
  ];

  options.services.mosquitto = {
    enable = lib.mkEnableOption "mosquitto MQTT broker";

    package = lib.mkOption {
      type = types.package;
      description = "Which package of mosquitto to use";
      default = pkgs.mosquitto;
      defaultText = lib.literalExpression "pkgs.mosquitto";
    };

    bind = lib.mkOption {
      type = types.nullOr types.str;
      default = "127.0.0.1";
      description = ''
        The IP interface to bind to.
        `null` means "all interfaces".
      '';
      example = "127.0.0.1";
    };

    port = lib.mkOption {
      type = types.port;
      default = 1883;
      description = "The TCP port to accept MQTT connections.";
    };

    extraConfig = lib.mkOption {
      type = types.lines;
      default = "";
      description = "Additional text to append to `mosquitto.conf`.";
      example = ''
        max_queued_messages 1000
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [ cfg.package ];

    env = {
      MOSQUITTO_PORT = allocatedPort;
      MOSQUITTO_HOST = cfg.bind;
    };

    processes.mosquitto = {
      ports.main.allocate = basePort;
      exec = "exec ${cfg.package}/bin/mosquitto -c ${configFile}";
    };
  };
}
