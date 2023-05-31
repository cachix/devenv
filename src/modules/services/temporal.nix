{ pkgs, lib, config, ... }:

let
  cfg = config.services.temporal;
  types = lib.types;

  databaseFile = config.env.DEVENV_STATE + "/temporal.sqlite";

  commandArgs = [
    "--log-format=pretty"
    "--ip=${cfg.ip}"
    "--port=${toString cfg.port}"
    "--headless=${lib.boolToString (!cfg.ui.enable)}"
    "--ui-ip=${cfg.ui.ip}"
    "--ui-port=${toString cfg.ui.port}"
  ] ++
  (lib.forEach cfg.namespaces (namespace: "--namespace=${namespace}")) ++
  (lib.optionals (!cfg.state.ephemeral) [ "--db-filename=${databaseFile}" ]) ++
  (lib.mapAttrsToList (name: value: "--sqlite-pragma ${name}=${value}") cfg.state.sqlite-pragma);
in
{
  options.services.temporal = {
    enable = lib.mkEnableOption "Temporal process";

    package = lib.mkOption {
      type = types.package;
      description = "Which package of Temporal to use.";
      default = pkgs.temporal-cli;
      defaultText = lib.literalExpression "pkgs.temporal-cli";
    };

    ip = lib.mkOption {
      type = types.str;
      default = "127.0.0.1";
      description = "IPv4 address to bind the frontend service to.";
    };

    port = lib.mkOption {
      type = types.port;
      default = 7233;
      description = "Port for the frontend gRPC service.";
    };

    ui = lib.mkOption {
      type = types.submodule {
        options = {
          enable = lib.mkOption {
            type = types.bool;
            default = true;
            description = "Enable the Web UI.";
          };

          ip = lib.mkOption {
            type = types.str;
            default = cfg.ip;
            description = "IPv4 address to bind the Web UI to.";
          };

          port = lib.mkOption {
            type = types.port;
            default = cfg.port + 1000;
            defaultText = lib.literalMD "[`services.temporal.port`](#servicestemporalport) + 1000";
            description = "Port for the Web UI.";
          };
        };
      };
      default = { };
      description = "UI configuration.";
    };

    namespaces = lib.mkOption {
      type = types.listOf types.str;
      default = [ ];
      description = "Specify namespaces that should be pre-created (namespace \"default\" is always created).";
      example = [
        "my-namespace"
        "my-other-namespace"
      ];
    };

    state = lib.mkOption {
      type = types.submodule {
        options = {
          ephemeral = lib.mkOption {
            type = types.bool;
            default = true;
            description = "When enabled, the Temporal state gets lost when the process exists.";
          };

          sqlite-pragma = lib.mkOption {
            type = types.attrsOf types.str;
            default = { };
            description = "Sqlite pragma statements";
            example = {
              journal_mode = "wal";
              synchronous = "2";
            };
          };
        };
      };
      default = { };
      description = "State configuration.";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [ cfg.package ];
    processes.temporal.exec = "${cfg.package}/bin/temporal server start-dev ${lib.concatStringsSep " " commandArgs}";
  };
}
