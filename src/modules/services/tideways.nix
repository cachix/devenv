{ pkgs, lib, config, ... }:

let
  cfg = config.services.tideways;

  socket = "${config.env.DEVENV_STATE}/tideways/tidewaysd.sock";

  startScript = pkgs.writeShellScriptBin "start-tideways" ''
    set -euo pipefail

    if [[ ! -d "${config.env.DEVENV_STATE}/tideways" ]]; then
      mkdir -p "${config.env.DEVENV_STATE}/tideways"
    fi

    exec ${cfg.daemonPackage}/bin/tideways-daemon -address ${socket} --env ${cfg.environment}
  '';
in
{
  options.services.tideways = {
    enable = lib.mkEnableOption ''
      Tideways profiler daemon

      It automatically installs Tideways PHP extension.
    '';

    apiKey = lib.mkOption {
      type = lib.types.str;
      description = ''
        Sets the API-Key for the Tideways Daemon.
      '';
      default = "";
    };

    service = lib.mkOption {
      type = lib.types.str;
      description = ''
        Sets the Service name for Tideways Daemon.
      '';
      default = "";
    };

    environment = lib.mkOption {
      type = lib.types.str;
      description = ''
        Sets the Environment for Tideways Daemon.
      '';
      default = "devenv";
    };

    profilingSpace = lib.mkOption {
      type = lib.types.bool;
      description = ''
        When the profiling space is enabled, the default monitoring will be disabled.
      '';
      default = true;
    };

    daemonPackage = lib.mkOption {
      type = lib.types.package;
      description = "Which package of tideways-daemon to use";
      default = pkgs.tideways-daemon;
      defaultText = lib.literalExpression "pkgs.tideways-daemon";
    };

    cliPackage = lib.mkOption {
      type = lib.types.package;
      description = "Which package of tideways-cli to use";
      default = pkgs.tideways-cli;
      defaultText = lib.literalExpression "pkgs.tideways-cli";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.cliPackage
    ];

    processes.tideways-daemon.exec = "${startScript}/bin/start-tideways";

    languages.php.ini = ''
      tideways.api_key=${cfg.apiKey}
      tideways.service=${cfg.service}
      tideways.connection=unix://${socket}
      ${lib.optionalString cfg.profilingSpace "tideways.monitor=none"}
    '';
  };
}
