{ pkgs, lib, config, ... }:

let
  cfg = config.services.mailpit;
  types = lib.types;

  # Port allocation: extract port from address strings
  parsePort = addr: lib.toInt (lib.last (lib.splitString ":" addr));
  parseHost = addr: lib.head (lib.splitString ":" addr);

  # A unix socket address ends in its permissions, not a port, so it is passed
  # through rather than parsed.
  isUnixAddress = addr: lib.hasPrefix "unix:" addr;

  uiIsUnix = isUnixAddress cfg.uiListenAddress;
  smtpIsUnix = isUnixAddress cfg.smtpListenAddress;

  uiAddr =
    if uiIsUnix
    then cfg.uiListenAddress
    else "${parseHost cfg.uiListenAddress}:${toString config.processes.mailpit.ports.ui.value}";
  smtpAddr =
    if smtpIsUnix
    then cfg.smtpListenAddress
    else "${parseHost cfg.smtpListenAddress}:${toString config.processes.mailpit.ports.smtp.value}";
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
      description = ''
        Listen address for UI.

        Either `<host>:<port>`, where the port is the base devenv allocates
        from, or a unix socket as `unix:<path>:<permissions>`, which is passed
        to mailpit as given. Mailpit requires the permissions.
      '';
      default = "127.0.0.1:8025";
      example = lib.literalExpression "\"unix:\${config.env.DEVENV_RUNTIME}/mailpit-ui.sock:660\"";
    };

    smtpListenAddress = lib.mkOption {
      type = types.str;
      description = ''
        Listen address for SMTP.

        Either `<host>:<port>`, where the port is the base devenv allocates
        from, or a unix socket as `unix:<path>:<permissions>`, which is passed
        to mailpit as given. Mailpit requires the permissions.
      '';
      default = "127.0.0.1:1025";
      example = lib.literalExpression "\"unix:\${config.env.DEVENV_RUNTIME}/mailpit-smtp.sock:660\"";
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

  config = lib.mkIf cfg.enable (lib.mkMerge [
    {
      # For `sendmail`
      packages = [ cfg.package ];

      tasks."devenv:mailpit:setup" = {
        exec = ''mkdir -p "$DEVENV_STATE/mailpit"'';
        before = [ "devenv:processes:mailpit" ];
      };

      processes.mailpit.exec = "exec ${cfg.package}/bin/mailpit --db-file $DEVENV_STATE/mailpit/db.sqlite3 --listen ${lib.escapeShellArg uiAddr} --smtp ${lib.escapeShellArg smtpAddr} ${lib.escapeShellArgs cfg.additionalArgs}";
    }

    # An undeclared port is one devenv never allocates.
    (lib.mkIf (!uiIsUnix) {
      processes.mailpit.ports.ui.allocate = parsePort cfg.uiListenAddress;
    })
    (lib.mkIf (!smtpIsUnix) {
      processes.mailpit.ports.smtp.allocate = parsePort cfg.smtpListenAddress;
    })
  ]);
}
