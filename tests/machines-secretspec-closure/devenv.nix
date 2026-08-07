{ pkgs, ... }:

{
  machines.resolver = {
    system = pkgs.stdenv.hostPlatform.system;
    target.host = "root@resolver.example.com";
    hardware.facter = null;

    install.secretspec = {
      execution = "target";
      extraPackages = targetPkgs: [ targetPkgs.coreutils ];
    };
    install.secrets."/var/lib/bootstrap/token" = {
      secret = "BOOTSTRAP_TOKEN";
      owner = "0:0";
      mode = "0600";
    };

    nixos = {
      system.stateVersion = "24.11";
      networking.hostName = "resolver-test";
      fileSystems."/" = {
        device = "/dev/disk/by-label/nixos";
        fsType = "ext4";
      };
      boot.loader.grub.devices = [ "nodev" ];
      users.users.root.openssh.authorizedKeys.keys = [
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKmachinesSecretspecClosureTestOnly"
      ];
    };
  };

  machines.unauthenticated = {
    system = pkgs.stdenv.hostPlatform.system;
    target.host = "root@unauthenticated.example.com";
    hardware.facter = null;
    nixos = {
      system.stateVersion = "24.11";
      networking.hostName = "unauthenticated-test";
    };
  };
}
