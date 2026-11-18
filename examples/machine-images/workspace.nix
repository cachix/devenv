{
  pkgs,
  lib,
  inputs,
  ...
}:
let
  # Evaluate the shared environment independently of the image definitions,
  # using the guest's packages and paths rather than the build workstation's.
  workspace = lib.evalModules {
    class = "devenv";
    specialArgs = { inherit pkgs inputs; };
    modules = [
      (inputs.devenv.modules + /top-level.nix)
      ./project/devenv.nix
      {
        devenv.root = "/home/dev/project";
        devenv.tmpdir = "/tmp";
        devenv.runtime = "/home/dev/.devenv-runtime";
        devenv.flakesIntegration = true;
      }
    ];
  };
  shell = inputs.mk-shell-bin.lib.mkShellBin {
    drv = workspace.config.shell;
    nixpkgs = pkgs;
  };
  # Only this small, explicit source directory enters the image. Never copy
  # the workstation's .devenv directory, credentials, or unfiltered repository.
  projectSource = lib.fileset.toSource {
    root = ./project;
    fileset = lib.fileset.unions [
      ./project/devenv.nix
      ./project/hello.py
    ];
  };
  launcher = pkgs.writeShellScriptBin "project-shell" ''
    cd /home/dev/project || exit 1
    if [ "$#" -gt 0 ]; then
      source ${shell.envScript}
      exec "$@"
    fi
    exec ${pkgs.bashInteractive}/bin/bash --rcfile ${shell.envScript} -i
  '';
in
{
  system.stateVersion = "26.05";
  users.users.dev = {
    isNormalUser = true;
    home = "/home/dev";
    shell = pkgs.bashInteractive;
  };
  # The launcher references the captured shell environment, so its closure
  # is installed and remains reachable during garbage collection.
  environment.systemPackages = [
    launcher
    pkgs.devenv
  ];
  systemd.services.devenv-workspace = {
    description = "Initialize the prebaked development workspace";
    wantedBy = [ "multi-user.target" ];
    before = [ "sshd.service" ];
    after = [ "local-fs.target" ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    script = ''
      if [ ! -e /home/dev/project ]; then
        install -d -m 0755 -o dev -g users /home/dev
        workspace=$(mktemp -d /home/dev/.project.XXXXXX)
        cp -r ${projectSource}/. "$workspace/"
        chmod -R u+w "$workspace"
        chown -R dev:users "$workspace"
        mv "$workspace" /home/dev/project
      fi
    '';
  };
}
