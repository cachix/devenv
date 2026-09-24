{ config, lib, ... }:
{
  outputs.facts =
    config.machines.server._nixosEval.config.environment.etc."devenv/machine-facts.json".source;
  machines.server = {
    target.host = "root@check.invalid";
    hardware.facter = null;
    nixos = {
      services.openssh = {
        enable = true;
        ports = [ 2222 ];
        settings.PermitRootLogin = "prohibit-password";
        authorizedKeysInHomedir = false;
      };
      networking.firewall.allowedTCPPortRanges = [
        {
          from = 2200;
          to = 2300;
        }
      ];
      users.users.root.openssh.authorizedKeys.keys = [ "ssh-ed25519 AAAATEST public-fixture" ];
      boot.loader.grub.devices = [ "nodev" ];
      fileSystems."/" = {
        device = "/dev/disk/by-label/root";
        fsType = "ext4";
      };
      system.stateVersion = "25.11";
    };
    build.nixos = lib.mkForce (throw "check must not build the system");
    build.deployer = lib.mkForce (throw "check must not build the executor");
  };
  machines.unrelated = {
    hardware.facter = null;
    home-manager = { ... }: { impossible = throw "unrelated role forced"; };
  };
}
