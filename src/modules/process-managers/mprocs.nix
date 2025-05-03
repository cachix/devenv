{ pkgs, config, lib, ... }:
let
  cfg = config.process.managers.mprocs;
  settingsFormat = pkgs.formats.yaml { };
  makeImpurePackage = impurePath:
    pkgs.runCommandLocal
      "${lib.strings.sanitizeDerivationName impurePath}-impure"
      {
        __impureHostDeps = [ impurePath ];
      } "mkdir -p $out/bin && ln -s ${impurePath} $out/bin";
in
{
  options.process.managers.mprocs = {
    enable = lib.mkEnableOption "mprocs as the process manager" // {
      internal = true;
    };

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.mprocs;
      defaultText = lib.literalExpression "pkgs.mprocs";
      description = "The mprocs package to use.";
    };

    configFile = lib.mkOption {
      type = lib.types.path;
      internal = true;
    };

    settings = lib.mkOption {
      type = settingsFormat.type;
      description = ''
        Top-level mprocs.yaml options

        https://github.com/pvolok/mprocs?tab=readme-ov-file#config
      '';
      default = { };
    };
  };

  config = lib.mkIf cfg.enable {
    process.manager.args = { "config" = cfg.configFile; };

    process.manager.command = lib.mkDefault ''
      ${cfg.package}/bin/mprocs \
        ${lib.cli.toGNUCommandLineShell { } config.process.manager.args}
    '';

    packages = [ cfg.package ] ++ lib.optionals pkgs.stdenv.isDarwin
      [ (makeImpurePackage "/usr/bin/pbcopy") ];

    process.managers.mprocs = {
      configFile =
        lib.mkDefault (settingsFormat.generate "mprocs.yaml" cfg.settings);
      settings = {
        procs = lib.mapAttrs
          (name: value:
            let scriptPath = pkgs.writeShellScript name value.exec;
            in { cmd = [ "${scriptPath}" ]; })
          config.processes;
      };
    };
  };
}
