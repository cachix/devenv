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

    capabilities = lib.mkOption {
      type = lib.types.attrsOf lib.types.bool;
      internal = true;
      readOnly = true;
      description = "Capabilities provided by the mprocs process manager.";
      default = {
        background_start = false;
        devenv_attach = false;
        wait_ready = false;
        individual_control = false;
        subset_start = false;
        requires_tty = true;
        semantic_shutdown = false;
      };
    };

    shutdownScript = lib.mkOption {
      type = lib.types.nullOr lib.types.package;
      internal = true;
      readOnly = true;
      default = null;
      description = "Manager-specific graceful shutdown script, if one is available.";
    };
  };

  config = lib.mkIf cfg.enable {
    process.manager.args = { "config" = cfg.configFile; };

    process.manager.command = lib.mkDefault ''
      ${lib.getExe cfg.package} \
        ${(lib.cli.toCommandLineShellGNU or lib.cli.toGNUCommandLineShell) { } config.process.manager.args}
    '';

    packages = [ cfg.package ] ++ lib.optionals pkgs.stdenv.hostPlatform.isDarwin
      [ (makeImpurePackage "/usr/bin/pbcopy") ];

    process.managers.mprocs = {
      configFile =
        lib.mkDefault (settingsFormat.generate "mprocs.yaml" cfg.settings);
      settings = {
        procs =
          lib.mapAttrs
            (
              name: value:
                {
                  # Run through devenv-tasks to support before/after task dependencies
                  cmd = [ "bash" "-c" config.process.taskCommands.${name} ];
                }
                // lib.optionalAttrs (lib.hasAttr "cwd" value && value.cwd != null) { cwd = value.cwd; }
            )
            config.processes;
      };
    };
  };
}
