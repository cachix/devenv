{ pkgs
, config
, lib
, ...
}:
let
  cfg = config.process.managers.overmind;
  processManagerTypes = import ../lib/process-manager-types.nix { inherit lib; };

  # tmux 3.7c requires an explicit allocator choice on Darwin. Some nixpkgs
  # revisions contain 3.7c without the corresponding packaging fix:
  # https://github.com/NixOS/nixpkgs/commit/56d4d710bd0bc7bcf953f6616bdc3e48c21ec549
  needsDarwinTmuxAllocator =
    pkgs.stdenv.hostPlatform.isDarwin
    && !lib.any (flag: flag == "--enable-jemalloc" || flag == "--disable-jemalloc") (
      pkgs.tmux.configureFlags or [ ]
    );
  tmuxPackage =
    if needsDarwinTmuxAllocator then
      pkgs.tmux.overrideAttrs
        (oldAttrs: {
          buildInputs = (oldAttrs.buildInputs or [ ]) ++ [ pkgs.jemalloc ];
          configureFlags = (oldAttrs.configureFlags or [ ]) ++ [ "--enable-jemalloc" ];
        })
    else
      pkgs.tmux;
  defaultOvermindPackage =
    if needsDarwinTmuxAllocator then pkgs.overmind.override { tmux = tmuxPackage; } else pkgs.overmind;
in
{
  options.process.managers.overmind = {
    enable = lib.mkEnableOption "overmind as the process manager" // {
      internal = true;
    };

    package = lib.mkOption {
      type = lib.types.package;
      default = defaultOvermindPackage;
      defaultText = lib.literalExpression "pkgs.overmind";
      description = "The overmind package to use.";
    };

    capabilities = lib.mkOption {
      type = processManagerTypes.capabilities;
      internal = true;
      readOnly = true;
      description = "Capabilities provided by the overmind process manager.";
      default = {
        background_start = true;
        devenv_attach = false;
        wait_ready = false;
        individual_control = false;
        cold_start_subset = true;
      };
    };

    adapter = lib.mkOption {
      type = processManagerTypes.adapter;
      internal = true;
      readOnly = true;
      default = {
        terminal = "none";
        stop = "command";
        client = "none";
      };
      description = "Runtime adapter settings of the overmind process manager.";
    };

    stopCommand = lib.mkOption {
      type = processManagerTypes.stopCommand;
      internal = true;
      readOnly = true;
      description = "Manager-specific graceful stop command, if one is available.";
      default = pkgs.writeShellScript "devenv-overmind-shutdown" ''
        set -u

        status=0
        ${lib.getExe cfg.package} quit \
          --socket ${lib.escapeShellArg "${config.devenv.runtime}/overmind.sock"} || status=$?
        [ "$status" -eq 0 ] || exit "$status"

        # Overmind drops its own tmux session as it exits, but never the server
        # that held it, so every run leaves an idle server behind. The server
        # runs in its own session, so the process scope cannot reach it either.
        #
        # Its socket is named `overmind-<session>-<id>`, where <session> is the
        # project directory name with every character other than a letter or a
        # digit turned into a dash, runs of dashes collapsed, lowercased.
        session=$(printf '%s' ${lib.escapeShellArg (baseNameOf config.devenv.root)} \
          | ${lib.getExe' pkgs.gnused "sed"} -e 's/[^a-zA-Z0-9]/-/g' -e 's/-\{2,\}/-/g' \
          | ${lib.getExe' pkgs.coreutils "tr"} 'A-Z' 'a-z')
        tmux_dir="''${TMUX_TMPDIR:-/tmp}/tmux-$(${lib.getExe' pkgs.coreutils "id"} -u)"

        # Take down only a server that no longer holds that session. One that
        # still does belongs to an overmind that has not finished, which may be
        # another project of the same name. A server we cannot talk to is left
        # alone for the same reason.
        for tmux_socket in "$tmux_dir"/overmind-"$session"-*; do
          [ -S "$tmux_socket" ] || continue

          for _ in $(${lib.getExe' pkgs.coreutils "seq"} 1 100); do
            sessions=$(${lib.getExe tmuxPackage} -S "$tmux_socket" \
              list-sessions -F '#{session_name}' 2>/dev/null) || continue 2

            if printf '%s\n' "$sessions" \
              | ${lib.getExe' pkgs.gnugrep "grep"} -qxF "$session"; then
              ${lib.getExe' pkgs.coreutils "sleep"} 0.1
            else
              ${lib.getExe tmuxPackage} -S "$tmux_socket" kill-server \
                2>/dev/null || true
              continue 2
            fi
          done
        done
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    process.manager.args = {
      "root" = config.devenv.root;
      "socket" = "${config.devenv.runtime}/overmind.sock";
      "procfile" = config.procfile;
    };

    process.manager.command = lib.mkDefault ''
      ${lib.getExe cfg.package} start \
        ${
          (lib.cli.toCommandLineShellGNU or lib.cli.toGNUCommandLineShell) { } config.process.manager.args
        } \
        "$@" &
    '';

    packages = [ cfg.package ];
  };
}
