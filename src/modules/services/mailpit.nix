{ pkgs, lib, config, ... }:

let
  cfg = config.services.mailpit;
  types = lib.types;
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
    processes.mailpit.exec = "${cfg.package}/bin/mailpit --listen ${cfg.uiListenAddress} --smtp ${cfg.smtpListenAddress} ${lib.concatStringsSep " " cfg.additionalArgs}";
  };
}
