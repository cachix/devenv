{ pkgs, lib, config, ... }:

let
  cfg = config.services.influxdb;
  types = lib.types;

  # Port allocation
  basePort = cfg.port;
  allocatedPort = config.processes.influxdb-server.ports.main.value;

  startScript = pkgs.writeShellScriptBin "start-influxdb" ''
    set -euo pipefail
    INFLUXDB_DATA="${config.env.DEVENV_STATE}/influxdb"
    mkdir -p "$INFLUXDB_DATA"
    exec ${cfg.package}/bin/influxd \
      --bolt-path="$INFLUXDB_DATA/influxd.bolt" \
      --engine-path="$INFLUXDB_DATA/engine" \
      --http-bind-address=":${toString allocatedPort}" \
      ${lib.concatStringsSep " " cfg.extraArgs}
  '';
in
{
  options.services.influxdb = {
    enable = lib.mkEnableOption "influxdb";

    package = lib.mkOption {
      type = types.package;
      description = "Which package of InfluxDB server to use";
      default = pkgs.influxdb2-server;
      defaultText = lib.literalExpression "pkgs.influxdb2-server";
    };

    port = lib.mkOption {
      type = types.port;
      default = 8086;
      description = "The TCP port for the InfluxDB HTTP API.";
    };

    extraArgs = lib.mkOption {
      type = types.listOf types.str;
      default = [ ];
      example = [ "--flux-log-enabled" ];
      description = "Additional arguments passed to `influxd` during startup.";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      pkgs.influxdb2-cli
    ];

    env.INFLUX_HOST = "http://localhost:${toString allocatedPort}";

    processes.influxdb-server = {
      ports.main.allocate = basePort;
      exec = "${startScript}/bin/start-influxdb";

      ready = {
        exec = "${pkgs.curl}/bin/curl -sf http://localhost:${toString allocatedPort}/health";
        initial_delay = 2;
        period = 10;
        probe_timeout = 4;
        success_threshold = 1;
        failure_threshold = 5;
      };
    };
  };
}
