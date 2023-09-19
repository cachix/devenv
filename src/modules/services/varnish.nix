{ pkgs, lib, config, ... }:

with lib;

let
  cfg = config.services.varnish;
  cfgFile = pkgs.writeText "varnish.vcl" cfg.vcl;
  workingDir = "${config.env.DEVENV_STATE}/varnish";
in
{
  options.services.varnish = {
    enable = mkEnableOption "Varnish process and expose utilities";

    package = mkOption {
      type = types.package;
      description = "Which Varnish package to use.";
      default = pkgs.varnish;
      defaultText = lib.literalExpression "pkgs.varnish";
    };

    memorySize = mkOption {
      type = types.str;
      description = "How much memory to allocate to Varnish.";
      default = "64M";
    };

    listen = mkOption {
      type = types.str;
      description = "Which address to listen on.";
      default = "127.0.0.1:6081";
    };

    vcl = mkOption {
      type = types.lines;
      description = "Varnish VCL configuration.";
      default = ''
        vcl 4.0;

        backend default {
          .host = "127.0.0.1";
          .port = "80";
        }
      '';
    };

    extraModules = mkOption {
      type = types.listOf types.package;
      default = [ ];
      example = literalExpression "[ pkgs.varnish73Packages.modules ]";
      description = lib.mdDoc ''
        Varnish modules (except 'std').
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    processes.varnish.exec = "${cfg.package}/bin/varnishd -n ${workingDir} -F -f ${cfgFile} -s malloc,${toString cfg.memorySize} -a ${cfg.listen} ${lib.optionalString (cfg.extraModules != []) " -p vmod_path='${lib.makeSearchPathOutput "lib" "lib/varnish/vmods" ([cfg.package] ++ cfg.extraModules)}' -r vmod_path"}";

    scripts.varnishadm.exec = "exec ${cfg.package}/bin/varnishadm -n ${workingDir} $@";
    scripts.varnishtop.exec = "exec ${cfg.package}/bin/varnishtop -n ${workingDir} $@";
    scripts.varnishhist.exec = "exec ${cfg.package}/bin/varnishhist -n ${workingDir} $@";
    scripts.varnishlog.exec = "exec ${cfg.package}/bin/varnishlog -n ${workingDir} $@";
    scripts.varnishstat.exec = "exec ${cfg.package}/bin/varnishstat -n ${workingDir} $@";
  };
}
