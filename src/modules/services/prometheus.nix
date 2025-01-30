{ pkgs, lib, config, ... }:

let
  cfg = config.services.prometheus;

  configFile = pkgs.writeText "prometheus.yml" (lib.generators.toYAML { } (
    {
      global = cfg.globalConfig;
      scrape_configs = cfg.scrapeConfigs;
    } // lib.optionalAttrs (cfg.ruleFiles != [ ]) {
      rule_files = cfg.ruleFiles;
    } // lib.optionalAttrs (cfg.alerting != null) {
      alerting = cfg.alerting;
    } // lib.optionalAttrs (cfg.remoteWrite != [ ]) {
      remote_write = cfg.remoteWrite;
    } // lib.optionalAttrs (cfg.remoteRead != [ ]) {
      remote_read = cfg.remoteRead;
    } // lib.optionalAttrs (cfg.advanced.storage != { }) {
      storage = cfg.advanced.storage;
    } // lib.optionalAttrs (cfg.advanced.tsdb != { }) {
      tsdb = cfg.advanced.tsdb;
    }
  ));
  prometheusArgs = lib.concatStringsSep " " ([
    "--config.file=${cfg.configFile}"
    "--storage.tsdb.path=${cfg.storage.path}"
    "--storage.tsdb.retention.time=${cfg.storage.retentionTime}"
    "--web.listen-address=:${toString cfg.port}"
  ]
  ++ lib.optional cfg.experimentalFeatures.enableExemplars "--enable-feature=exemplar-storage"
  ++ lib.optional cfg.experimentalFeatures.enableTracing "--enable-feature=tracing"
  ++ lib.optional cfg.experimentalFeatures.enableOTLP "--enable-feature=otlp-write-receiver"
  ++ lib.optional (cfg.extraArgs != "") cfg.extraArgs);
in
{
  options.services.prometheus = {
    enable = lib.mkEnableOption "Prometheus monitoring system";

    package = lib.mkOption {
      type = lib.types.package;
      description = "Which package of Prometheus to use";
      default = pkgs.prometheus;
      defaultText = lib.literalExpression "pkgs.prometheus";
    };

    storage = {
      path = lib.mkOption {
        type = lib.types.str;
        default = "${config.env.DEVENV_STATE}/prometheus";
        description = "Path where Prometheus will store its database";
      };

      retentionTime = lib.mkOption {
        type = lib.types.str;
        default = "15d";
        description = "How long to retain data";
      };
    };

    port = lib.mkOption {
      type = lib.types.port;
      default = 9090;
      description = "Port for Prometheus web interface";
    };

    globalConfig = lib.mkOption {
      type = lib.types.attrs;
      default = {
        scrape_interval = "1m";
        scrape_timeout = "10s";
        evaluation_interval = "1m";
      };
      description = "Global Prometheus configuration";
    };

    scrapeConfigs = lib.mkOption {
      type = lib.types.listOf lib.types.attrs;
      default = [ ];
      description = "List of scrape configurations";
    };

    ruleFiles = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      description = "List of rule files to load";
    };

    alerting = lib.mkOption {
      type = lib.types.nullOr lib.types.attrs;
      default = null;
      description = "Alerting configuration";
    };

    remoteWrite = lib.mkOption {
      type = lib.types.listOf lib.types.attrs;
      default = [ ];
      description = "Remote write configurations";
    };

    remoteRead = lib.mkOption {
      type = lib.types.listOf lib.types.attrs;
      default = [ ];
      description = "Remote read configurations";
    };

    experimentalFeatures = {
      enableExemplars = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = "Enable exemplar storage";
      };

      enableTracing = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = "Enable tracing";
      };

      enableOTLP = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = "Enable OTLP receiver";
      };
    };

    advanced = {
      storage = lib.mkOption {
        type = lib.types.attrs;
        default = { };
        description = "Storage configuration";
      };

      tsdb = lib.mkOption {
        type = lib.types.attrs;
        default = { };
        description = "TSDB configuration";
      };
    };

    extraArgs = lib.mkOption {
      type = lib.types.str;
      default = "";
      description = "Additional arguments to pass to Prometheus";
    };

    configFile = lib.mkOption {
      type = lib.types.path;
      default = configFile;
      internal = true;
      description = "The generated Prometheus configuration file";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ];

    processes.prometheus = {
      exec = "${cfg.package}/bin/prometheus ${prometheusArgs}";

      process-compose = {
        readiness_probe = {
          http_get = {
            host = "127.0.0.1";
            port = cfg.port;
            path = "/-/ready";
          };
          initial_delay_seconds = 2;
          period_seconds = 10;
          timeout_seconds = 4;
          success_threshold = 1;
          failure_threshold = 3;
        };

        availability.restart = "on_failure";
      };
    };

    enterShell = ''
      mkdir -p "${cfg.storage.path}"
      chmod 700 "${cfg.storage.path}"
    '';
  };
}
