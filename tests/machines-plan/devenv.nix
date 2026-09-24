{
  pkgs,
  lib,
  config,
  ...
}:
{
  outputs.test-python = pkgs.python3;
  machines.server = {
    target.host = "reader@preview.invalid";
    target.sshOpts = [
      "-p"
      "2222"
    ];
    hardware.facter = null;
    nixos = {
      services.openssh.enable = true;
      boot.loader.grub.devices = [ "nodev" ];
      fileSystems."/" = {
        device = "/dev/disk/by-label/root";
        fsType = "ext4";
      };
      system.stateVersion = "25.11";
    };
    build.nixos = lib.mkForce (
      pkgs.runCommand "preview-system" { } ''
        mkdir -p $out/bin $out/etc/devenv
        cp ${pkgs.writeText "preview-facts" (builtins.toJSON config.machines.server.deploy.facts)} $out/etc/devenv/machine-facts.json
        touch $out/bin/switch-to-configuration
        echo ${pkgs.writeText "preview-reference" "retained dependency"} > $out/reference
      ''
    );
    build.deployer = lib.mkForce (
      pkgs.runCommand "preview-executor" { } ''
        mkdir -p $out/bin
        touch $out/bin/devenv-machine-deploy
      ''
    );
  };
  machines.local = {
    hardware.facter = null;
    home-manager = { ... }: { throwIfEvaluated = throw "unselected local role was evaluated"; };
  };
}
