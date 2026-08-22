{ pkgs, ... }:

{
  machines.server = {
    system = pkgs.stdenv.hostPlatform.system;
    target.host = "root@server.example.com";
    hardware.facter = null;
    install.kexec.image = "https://example.com/kexec.tar.gz";
    nixos = {
      services.openssh.enable = true;
      users.users.root.openssh.authorizedKeys.keys = [ "ssh-ed25519 install-control-test" ];
    };
  };
}
