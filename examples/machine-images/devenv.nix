{ pkgs, inputs, ... }:
{
  imports = [ ./project/devenv.nix ];

  machines.dev = {
    systems = [
      "x86_64-linux"
      "aarch64-linux"
    ];
    nixos = import ./workspace.nix;
    images.amazon = { pkgs, lib, ... }: {
      virtualisation.diskSize = 16384;
      ec2.efi = lib.mkDefault pkgs.stdenv.hostPlatform.isAarch64;
    };
    images.google-compute = {
      virtualisation.diskSize = 16384;
    };
  };

  # Run explicitly with: devenv build outputs.workspace-vm
  outputs.workspace-vm = pkgs.testers.runNixOSTest {
    name = "devenv-prebaked-workspace";
    nodes.machine = {
      imports = [ ./workspace.nix ];
      _module.args.inputs = inputs;
    };
    testScript = ''
      machine.start()
      machine.wait_for_unit("devenv-workspace.service")
      machine.succeed("unshare --net -- runuser -u dev -- project-shell python hello.py | grep 'Hello from the prebaked'")
      machine.succeed("runuser -u dev -- project-shell env | grep '^PROJECT_SHELL_READY=1$'")
      machine.succeed("runuser -u dev -- project-shell env | grep '^DEVENV_ROOT=/home/dev/project$'")
      machine.succeed("runuser -u dev -- touch /home/dev/project/user-edit")
      machine.succeed("systemctl restart devenv-workspace.service")
      machine.succeed("test -e /home/dev/project/user-edit")
      machine.succeed("mv /home/dev/project /home/dev/project.saved")
      machine.fail("runuser -u dev -- project-shell true")
      machine.succeed("test ! -e /home/dev/project")
      machine.succeed("mv /home/dev/project.saved /home/dev/project")
      machine.succeed("nix-store --gc")
      machine.succeed("unshare --net -- runuser -u dev -- project-shell python hello.py | grep 'Hello from the prebaked'")
    '';
  };

}
