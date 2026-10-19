{ ... }:
{
  machines.server = {
    system = "x86_64-linux";
    target.host = "root@192.0.2.1";
    hardware.facter = null;
    install.encryptionKeys."/tmp/luks.key" = "secrets/luks.key";
    install.extraFiles."/etc/a.conf".source = "files/a.conf";
    nixos = { };
  };
}
