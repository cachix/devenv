{ pkgs, lib, config, ... }:

let
  cfg = config.services.mailpit;
  types = lib.types;

  # Port allocation: extract port from address strings
  parsePort = addr: lib.toInt (lib.last (lib.splitString ":" addr));
  parseHost = addr: lib.head (lib.splitString ":" addr);

  baseUiPort = parsePort cfg.uiListenAddress;
  baseSmtpPort = parsePort cfg.smtpListenAddress;
  allocatedUiPort = config.processes.mailpit.ports.ui.value;
  allocatedSmtpPort = config.processes.mailpit.ports.smtp.value;
  uiHost = parseHost cfg.uiListenAddress;
  smtpHost = parseHost cfg.smtpListenAddress;
  uiAddr = "${uiHost}:${toString allocatedUiPort}";
  smtpAddr = "${smtpHost}:${toString allocatedSmtpPort}";
in
{
  options.services.mailpit = {
    enable = lib.mkEnableOption "mailpit process";

    package = lib.mkOption {
      type = types.package;
      description = "Which package of mailpit to use";
      default = pkgs.mailpit;
      defaultText = lib.literalExpression "pkgs.mailpit";
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
      example = [ "--max=500" ];
      description = ''
        Additional arguments passed to `mailpit`.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    # For `sendmail`
    packages = [ cfg.package ];

    tasks."devenv:mailpit:setup" = {
      exec = ''mkdir -p "$DEVENV_STATE/mailpit"'';
      before = [ "devenv:processes:mailpit" ];
    };

    processes.mailpit.ports.ui.allocate = baseUiPort;
    processes.mailpit.ports.smtp.allocate = baseSmtpPort;
    processes.mailpit.exec = "exec ${cfg.package}/bin/mailpit --db-file $DEVENV_STATE/mailpit/db.sqlite3 --listen ${lib.escapeShellArg uiAddr} --smtp ${lib.escapeShellArg smtpAddr} ${lib.escapeShellArgs cfg.additionalArgs}";
  };
}
