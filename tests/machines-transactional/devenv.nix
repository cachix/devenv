{ pkgs, lib, ... }:
{
  machines.server = {
    target.host = "root@transaction.invalid";
    hardware.facter = null;
    nixos = {
      boot.loader.grub.devices = [ "nodev" ];
      fileSystems."/" = {
        device = "/dev/disk/by-label/root";
        fsType = "ext4";
      };
      system.stateVersion = "25.11";
    };
    build.nixos = lib.mkForce (
      pkgs.runCommand "transaction-test-system" { }
        "mkdir -p $out/bin; touch $out/bin/switch-to-configuration"
    );
    build.deployer = lib.mkForce (
      pkgs.runCommand "transaction-test-executor" { }
        "mkdir -p $out/bin; touch $out/bin/devenv-machine-deploy"
    );
  };
}
