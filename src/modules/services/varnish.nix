{ pkgs, lib, config, ... }:

with lib;

let
  cfg = config.services.varnish;
  cfgFile = pkgs.writeText "varnish.vcl" cfg.vcl;
  workingDir = "${config.env.DEVENV_STATE}/varnish";

  varnishAvailable = pkgs ? varnish && (builtins.tryEval pkgs.varnish.drvPath).success;
  defaultPackage = if varnishAvailable || !(pkgs ? vinyl-cache) then pkgs.varnish else pkgs.vinyl-cache;
  usesVinyl = (cfg.package.pname or "") == "vinyl-cache";
  command = suffix: "${cfg.package}/bin/${if usesVinyl then "vinyl" else "varnish"}${suffix}";
  vmodDirectory = if usesVinyl then "lib/vinyl/vmods" else "lib/varnish/vmods";

  # Port allocation: extract port from listen address or use default
  parsePort = addr: lib.toInt (lib.last (lib.splitString ":" addr));
  parseHost = addr: lib.head (lib.splitString ":" addr);

  basePort = parsePort cfg.listen;
  allocatedPort = config.processes.varnish.ports.main.value;
  host = parseHost cfg.listen;
  listenAddr = "${host}:${toString allocatedPort}";
in
{
  options.services.varnish = {
    enable = mkEnableOption "Varnish process and expose utilities";

    package = mkOption {
      type = types.package;
      description = "Which Varnish or Vinyl Cache package to use.";
      default = defaultPackage;
      defaultText = lib.literalMD "`pkgs.varnish` when available, otherwise `pkgs.vinyl-cache`";
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
      description = ''
        Varnish modules (except 'std').
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    changelogs = [
      {
        date = "2026-11-25";
        title = "Varnish falls back to Vinyl Cache";
        when = usesVinyl && !varnishAvailable;
        description = ''
          On Nixpkgs versions that have retired the Varnish package, this service
          uses Vinyl Cache. The service and its helper commands keep their
          existing names. Custom VMODs must be built for the selected package.
        '';
      }
    ];

    env.VARNISH_PORT = allocatedPort;
    processes.varnish.ports.main.allocate = basePort;
    processes.varnish.exec = "exec ${command "d"} -n ${workingDir} -F -f ${cfgFile} -s malloc,${toString cfg.memorySize} -a ${listenAddr} ${lib.optionalString (cfg.extraModules != []) " -p vmod_path='${lib.makeSearchPathOutput "lib" vmodDirectory ([cfg.package] ++ cfg.extraModules)}' -r vmod_path"}";

    scripts.varnishadm.exec = "exec ${command "adm"} -n ${workingDir} $@";
    scripts.varnishtop.exec = "exec ${command "top"} -n ${workingDir} $@";
    scripts.varnishhist.exec = "exec ${command "hist"} -n ${workingDir} $@";
    scripts.varnishlog.exec = "exec ${command "log"} -n ${workingDir} $@";
    scripts.varnishstat.exec = "exec ${command "stat"} -n ${workingDir} $@";
  };
}
