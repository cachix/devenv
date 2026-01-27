{ pkgs, lib, config, ... }:

let
  cfg = config.services.mailhog;
  types = lib.types;

  # Port allocation: extract port from address strings
  parsePort = addr: lib.toInt (lib.last (lib.splitString ":" addr));
  parseHost = addr: lib.head (lib.splitString ":" addr);

  baseApiPort = parsePort cfg.apiListenAddress;
  baseSmtpPort = parsePort cfg.smtpListenAddress;
  allocatedApiPort = config.processes.mailhog.ports.api.value;
  allocatedSmtpPort = config.processes.mailhog.ports.smtp.value;
  apiHost = parseHost cfg.apiListenAddress;
  uiHost = parseHost cfg.uiListenAddress;
  smtpHost = parseHost cfg.smtpListenAddress;
  apiAddr = "${apiHost}:${toString allocatedApiPort}";
  uiAddr = "${uiHost}:${toString allocatedApiPort}"; # UI shares port with API
  smtpAddr = "${smtpHost}:${toString allocatedSmtpPort}";
in
{
  options.services.mailhog = {
    enable = lib.mkEnableOption "mailhog process";

    package = lib.mkOption {
      type = types.package;
      description = "Which package of mailhog to use";
      default = pkgs.mailhog;
      defaultText = lib.literalExpression "pkgs.mailhog";
    };

    apiListenAddress = lib.mkOption {
      type = types.str;
      description = "Listen address for API.";
      default = "127.0.0.1:8025";
    };

    uiListenAddress = lib.mkOption {
      type = types.str;
      description = "Listen address for UI.";
      default = "127.0.0.1:8025";
    };

    smtpListenAddress = lib.mkOption {
      type = types.str;
      description = "Listen address for SMTP.";
      default = "127.0.0.1:1025";
    };

    additionalArgs = lib.mkOption {
      type = types.listOf types.lines;
      default = [ ];
      example = [ "-invite-jim" ];
      description = ''
        Additional arguments passed to `mailhog`.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    processes.mailhog.ports.api.allocate = baseApiPort;
    processes.mailhog.ports.smtp.allocate = baseSmtpPort;
    processes.mailhog.exec = "exec ${cfg.package}/bin/MailHog -api-bind-addr ${apiAddr} -ui-bind-addr ${uiAddr} -smtp-bind-addr ${smtpAddr} ${lib.concatStringsSep " " cfg.additionalArgs}";
  };
}
