{ pkgs, config, lib, ... }:
let
  cfg = config.process.managers.native;
in
{
  options.process.managers.native = {
    enable = lib.mkEnableOption "native Rust process manager" // {
      internal = true;
    };

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.bashInteractive;
      defaultText = lib.literalExpression "pkgs.bashInteractive";
      description = "The shell package to use for running processes.";
      internal = true;
    };
  };

  config = lib.mkIf cfg.enable {
    changelogs = [
      {
        date = "2026-03-12";
        title = "Native TUI no longer exits on the first Ctrl-C";
        when = cfg.enable;
        description = ''
          In the native process manager's interactive TUI, the first `Ctrl-C` now opens a quit prompt instead of immediately shutting down devenv.

          After the interrupt prompt appears, press `c` to keep running or `q`/`Ctrl-C` to quit the environment.
        '';
      }
    ];

    packages = [ cfg.package ];

    # The actual process manager command will be invoked from devenv.rs
    # We just need to provide the configuration via procfileScript
    process.manager.command = lib.mkDefault ''
      # Native process manager is invoked directly from devenv up
      # This script should not be reached
      echo "Native process manager should be invoked from devenv up" >&2
      exit 1
    '';
  };
}
