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

    # `devenv up` invokes the native task runner directly from Rust and never
    # builds procfileScript. But procfileScript is still used by the processes
    # container (containers.nix), `nix run .#devenv-up` (flake-compat.nix), and
    # `config.ci`. For those code paths, run all process tasks in a single
    # `devenv-tasks run` invocation so they share one NativeProcessManager;
    # the runner blocks in run_foreground until processes exit. Includes
    # processes with start.enable = false to match `devenv up` (the native
    # manager surfaces them as stopped).
    process.manager.command = lib.mkDefault ''
      ${config.task.package}/bin/devenv-tasks run \
        --task-file ${config.task.config} \
        --mode all \
        --cache-dir ${lib.escapeShellArg config.devenv.dotfile} \
        --runtime-dir ${lib.escapeShellArg config.devenv.runtime} \
        ${lib.concatMapStringsSep " " (name: "devenv:processes:${name}") (lib.attrNames config.processes)} &
    '';
  };
}
