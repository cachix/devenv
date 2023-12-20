{ pkgs, lib, config, ... }:

let
  cfg = config.services.influxdb;
  types = lib.types;
in
{
  options.services.influxdb = {
    enable = lib.mkEnableOption "influxdb";

    package = lib.mkOption {
      type = types.package;
      description = "An open-source distributed time series database";
      default = pkgs.influxdb;
      defaultText = lib.literalExpression "pkgs.influxdb";
    };

    config = lib.mkOption {
      type = types.lines;
      default = "";
      description = "Configuration for InfluxDB-server";
    };
  };

  config = lib.mkIf cfg.enable {
    processes.influxdb-server.exec = "${cfg.package}/bin/influxd -config ${pkgs.writeText "influxdb.conf" cfg.config}";
  };
}
