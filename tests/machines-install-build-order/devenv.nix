{ pkgs, lib, config, ... }:
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
    secrets = lib.recursiveUpdate
      (machine (pkgs.runCommand "replacement-system-with-secrets" { } ''
        mkdir -p $out/etc/devenv
        cp ${pkgs.writeText "runtime-secrets.json" config.machines.secrets._nixosEval.config.environment.etc."devenv/secrets.json".text} $out/etc/devenv/secrets.json
        cp ${pkgs.writeText "bootstrap-secrets.json" config.machines.secrets._nixosEval.config.environment.etc."devenv/bootstrap-secrets.json".text} $out/etc/devenv/bootstrap-secrets.json
      ''))
      {
        nixos.devenv.secrets.RUNTIME_TOKEN.secret = "SHARED_TOKEN";
        install.secrets = {
          "/var/lib/first/key".secret = "SHARED_TOKEN";
          "/var/lib/second/key".secret = "SHARED_TOKEN";
        };
      };
  };
}
