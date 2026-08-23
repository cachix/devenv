{ pkgs, config, lib, ... }:
let
  cfg = config.process.managers.honcho;
in
{
  options.process.managers.honcho = {
    enable = lib.mkEnableOption "honcho as the process manager" // {
      internal = true;
    };

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.honcho;
      defaultText = lib.literalExpression "pkgs.honcho";
      description = "The honcho package to use.";
    };

    capabilities = lib.mkOption {
      type = lib.types.attrsOf lib.types.bool;
      internal = true;
      readOnly = true;
      description = "Capabilities provided by the honcho process manager.";
      default = {
        background_start = true;
        devenv_attach = false;
        wait_ready = false;
        individual_control = false;
        subset_start = true;
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
      "f" = config.procfile;
    };

    process.manager.command = lib.mkDefault ''
      ${lib.getExe cfg.package} start \
        ${(lib.cli.toCommandLineShellGNU or lib.cli.toGNUCommandLineShell) {} config.process.manager.args} \
        "$@" &
    '';

    packages = [ cfg.package ];
  };
}
