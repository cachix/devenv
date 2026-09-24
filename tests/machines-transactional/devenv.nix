{
  pkgs,
  lib,
  config,
  ...
}:
{
  machines.server = {
    target.host = "root@transaction.invalid";
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
      pkgs.runCommand "transaction-test-system" { } ''
        mkdir -p $out/bin $out/etc/devenv
        touch $out/bin/switch-to-configuration
        cp ${pkgs.writeText "transaction-facts" (builtins.toJSON config.machines.server.deploy.facts)} $out/etc/devenv/machine-facts.json
      ''
    );
    build.deployer = lib.mkForce (
      pkgs.runCommand "transaction-test-executor" { }
        "mkdir -p $out/bin; touch $out/bin/devenv-machine-deploy"
    );
  };
}
