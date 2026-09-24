{ pkgs, lib, ... }:
let
  machine = system: {
    target.host = "root@install-order.invalid";
    hardware.facter = null;
    nixos = {
      users.users.root.openssh.authorizedKeys.keys = [ "fixture" ];
      boot.loader.grub.devices = [ "nodev" ];
      system.stateVersion = "25.11";
      disko.devices.disk.fixture = {
        type = "disk";
        device = "/dev/devenv-install-test-does-not-exist";
        content.type = "gpt";
        content.partitions.root = {
          size = "100%";
          content = {
            type = "filesystem";
            format = "ext4";
            mountpoint = "/";
          };
        };
      };
    };
    build.nixos = lib.mkForce system;
    build.diskoScript = lib.mkForce (pkgs.writeShellScript "mock-disko" "exit 0");
  };
in
{
  machines = {
    failing = machine (
      pkgs.runCommand "replacement-build-fails" { } ''
        echo "intentional replacement build failure" >&2
        exit 1
      ''
    );
    working = machine (pkgs.runCommand "replacement-system" { } "mkdir -p $out");
  };
}
