{ pkgs, config, lib, ... }:
let
  cfg = config.process.managers.hivemind;
in
{
  options.process.managers.hivemind = {
    enable = lib.mkEnableOption "hivemind as the process manager" // {
      internal = true;
    };

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.hivemind;
      defaultText = lib.literalExpression "pkgs.hivemind";
      description = "The hivemind package to use.";
    };

    capabilities = lib.mkOption {
      type = lib.types.attrsOf lib.types.bool;
      internal = true;
      readOnly = true;
      description = "Capabilities provided by the hivemind process manager.";
      default = {
        background_start = true;
        devenv_attach = false;
        wait_ready = false;
        individual_control = false;
        subset_start = false;
        requires_tty = false;
        manager_aware_stop = false;
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
    process.manager.args = {
      "print-timestamps" = true;
    };

    process.manager.command = lib.mkDefault ''
      ${lib.getExe cfg.package} \
        ${(lib.cli.toCommandLineShellGNU or lib.cli.toGNUCommandLineShell) {} config.process.manager.args} \
        "$@" ${config.procfile} &
    '';

    packages = [ cfg.package ];
  };
}
