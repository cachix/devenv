{ pkgs, config, lib, ... }:

let
  cfg = config.services.opentelemetry-collector;
  types = lib.types;

  settingsFormat = pkgs.formats.yaml { };

  defaultSettings = {
    extensions = {
      health_check = {
        endpoint = "localhost:13133";
      };
    };
    service = {
      extensions = [ "health_check" ];
    };
  };

  otelConfig =
    if cfg.configFile == null
    then settingsFormat.generate "otel-config.yaml" cfg.settings
    else cfg.configFile;
in
{
  options.services.opentelemetry-collector = {
    enable = lib.mkEnableOption "opentelemetry-collector";

    package = lib.mkOption {
      type = types.package;
      description = "The OpenTelemetry Collector package to use";
      default = pkgs.opentelemetry-collector-contrib;
      defaultText = lib.literalExpression "pkgs.opentelemetry-collector-contrib";
    };

    configFile = lib.mkOption {
      type = types.nullOr types.path;
      description = ''
        Override the configuration file used by OpenTelemetry Collector.
        By default, a configuration is generated from `services.opentelemetry-collector.settings`.

        If overriding, enable the `health_check` extension to allow process-compose to check whether the Collector is ready.
        Otherwise, disable the readiness probe by setting `processes.opentelemetry-collector.process-compose.readiness_probe = {};`.
      '';
      default = null;
      example = lib.literalExpression ''
        pkgs.writeTextFile { name = "otel-config.yaml"; text = "..."; }
      '';
    };

    settings = lib.mkOption {
      type = types.submodule ({ freeformType = settingsFormat.type; } // defaultSettings);
      description = ''
        OpenTelemetry Collector configuration.
        Refer to https://opentelemetry.io/docs/collector/configuration/
        for more information on how to configure the Collector.
      '';
      defaultText = defaultSettings;
    };
  };

  config = lib.mkIf cfg.enable {
    processes.opentelemetry-collector = {
      exec = "${lib.getExe cfg.package} --config ${otelConfig}";

      process-compose = {
        readiness_probe = {
          http_get = {
            host = "localhost";
            scheme = "http";
            path = "/";
            port = 13133;
          };
          initial_delay_seconds = 2;
          period_seconds = 10;
          timeout_seconds = 5;
          success_threshold = 1;
          failure_threshold = 3;
        };
        availability.restart = "on_failure";
      };
    };
  };
}
