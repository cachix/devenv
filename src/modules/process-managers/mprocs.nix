{ pkgs, config, lib, ... }:
let
  cfg = config.process.managers.mprocs;
  settingsFormat = pkgs.formats.yaml { };
  processManagerTypes = import ../lib/process-manager-types.nix { inherit lib; };
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
      type = processManagerTypes.capabilities;
      internal = true;
      readOnly = true;
      description = "Capabilities provided by the mprocs process manager.";
      default = {
        background_start = false;
        devenv_attach = false;
        wait_ready = false;
        individual_control = false;
        cold_start_subset = false;
      };
    };

    adapter = lib.mkOption {
      type = processManagerTypes.adapter;
      internal = true;
      readOnly = true;
      default = { terminal = "controlling"; stop = "process-scope"; client = "none"; };
      description = "Runtime adapter settings of the mprocs process manager.";
    };

    stopCommand = lib.mkOption {
      type = processManagerTypes.stopCommand;
      internal = true;
      readOnly = true;
      default = null;
      description = "Manager-specific graceful stop command, if one is available.";
    };
  };

  config = lib.mkIf cfg.enable {
    process.manager.args = { "config" = cfg.configFile; };

    process.manager.command = lib.mkDefault ''
      ${lib.getExe cfg.package} \
        ${(lib.cli.toCommandLineShellGNU or lib.cli.toGNUCommandLineShell) { } config.process.manager.args}
    '';

    packages = [ cfg.package ];

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
