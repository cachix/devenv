{ pkgs, lib, config, ... }:

let
  cfg = config.services.mailhog;
  types = lib.types;
in
{
  options.services.mailhog = {
    enable = lib.mkEnableOption "Add mailhog process.";

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
    processes.mailhog.exec = "${cfg.package}/bin/MailHog -api-bind-addr ${cfg.apiListenAddress} -ui-bind-addr ${cfg.uiListenAddress} -smtp-bind-addr ${cfg.smtpListenAddress} ${lib.concatStringsSep " " cfg.additionalArgs}";
  };
}
