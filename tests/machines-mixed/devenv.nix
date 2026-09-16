{
  pkgs,
  lib,
  config,
  ...
}:
let
  home =
    name:
    pkgs.writeTextFile {
      name = "mixed-home-${name}";
      destination = "/activate";
      executable = true;
      text = ''
        #!${pkgs.bash}/bin/bash
        echo local-activation >> "$MIXED_LOG"
      '';
    };
in
{
  machines.a-linux = {
    target.host = "root@linux.invalid";
    hardware.facter = null;
    nixos = {
      services.openssh.enable = true;
      boot.loader.grub.devices = [ "nodev" ];
      fileSystems."/" = {
        device = "/dev/root";
        fsType = "ext4";
      };
      system.stateVersion = "25.11";
    };
    home-manager = { ... }: { forbidden = throw "unexpected home evaluation"; };
    build.nixos = lib.mkForce (
      pkgs.runCommand "mixed-linux" { } ''
        mkdir -p $out/bin $out/etc/devenv
        touch $out/bin/switch-to-configuration
        cp ${pkgs.writeText "mixed-facts" (builtins.toJSON config.machines.a-linux.deploy.facts)} $out/etc/devenv/machine-facts.json
      ''
    );
    build.deployer = lib.mkForce (
      pkgs.runCommand "mixed-executor" { } ''
        mkdir -p $out/bin
        touch $out/bin/devenv-machine-deploy
      ''
    );
    build.home-manager = lib.mkForce (home "linux");
  };
  machines.b-mac = {
    system = "aarch64-darwin";
    target.host = "admin@mac.invalid";
    nix-darwin = { ... }: { forbidden = throw "unexpected darwin evaluation"; };
    home-manager = { ... }: { forbidden = throw "unexpected home evaluation"; };
    build.nix-darwin = lib.mkForce (
      pkgs.runCommand "mixed-darwin" { } "mkdir -p $out; touch $out/activate"
    );
    build.home-manager = lib.mkForce (home "mac");
  };
  machines.c-home = {
    target.host = "user@home.invalid";
    home-manager = { ... }: { forbidden = throw "unexpected home evaluation"; };
    build.home-manager = lib.mkForce (home "remote");
  };
  machines.d-local = {
    home-manager = { ... }: { forbidden = throw "unexpected home evaluation"; };
    build.home-manager = lib.mkForce (home "local");
  };
}
