{ pkgs, lib, config, ... }:

let
  cfg = config.services.meilisearch;
  types = lib.types;

in
{
  options.services.meilisearch = {
    enable = lib.mkEnableOption "Meilisearch";

    listenAddress = lib.mkOption {
      description = "Meilisearch listen address.";
      default = "127.0.0.1";
      type = types.str;
    };

    listenPort = lib.mkOption {
      description = "Meilisearch port to listen on.";
      default = 7700;
      type = types.port;
    };

    environment = lib.mkOption {
      description = "Defines the running environment of Meilisearch.";
      default = "development";
      type = types.enum [ "development" "production" ];
    };

    noAnalytics = lib.mkOption {
      description = ''
        Deactivates analytics.
        Analytics allow Meilisearch to know how many users are using Meilisearch,
        which versions and which platforms are used.
        This process is entirely anonymous.
      '';
      default = true;
      type = types.bool;
    };

    logLevel = lib.mkOption {
      description = ''
        Defines how much detail should be present in Meilisearch's logs.
        Meilisearch currently supports four log levels, listed in order of increasing verbosity:
        - 'ERROR': only log unexpected events indicating Meilisearch is not functioning as expected
        - 'WARN:' log all unexpected events, regardless of their severity
        - 'INFO:' log all events. This is the default value
        - 'DEBUG': log all events and including detailed information on Meilisearch's internal processes.
          Useful when diagnosing issues and debugging
      '';
      default = "INFO";
      type = types.str;
    };

    maxIndexSize = lib.mkOption {
      description = ''
        Sets the maximum size of the index.
        Value must be given in bytes or explicitly stating a base unit.
        For example, the default value can be written as 107374182400, '107.7Gb', or '107374 Mb'.
        Default is 100 GiB
      '';
      default = "107374182400";
      type = types.str;
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [ pkgs.meilisearch ];

    env.MEILI_DB_PATH = config.env.DEVENV_STATE + "/meilisearch";
    env.MEILI_HTTP_ADDR = "${cfg.listenAddress}:${toString cfg.listenPort}";
    env.MEILI_NO_ANALYTICS = toString cfg.noAnalytics;
    env.MEILI_ENV = cfg.environment;
    env.MEILI_DUMP_DIR = config.env.MEILI_DB_PATH + "/dumps";
    env.MEILI_LOG_LEVEL = cfg.logLevel;
    env.MEILI_MAX_INDEX_SIZE = cfg.maxIndexSize;

    processes.meilisearch = {
      exec = "${pkgs.meilisearch}/bin/meilisearch";
    };
  };
}
