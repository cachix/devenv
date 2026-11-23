{
  pkgs,
  inputs,
  lib,
  ...
}:
let
  system = pkgs.stdenv.hostPlatform.system;
  linuxSystem = lib.replaceStrings [ "darwin" ] [ "linux" ] system;
  darwinSystem = if pkgs.stdenv.hostPlatform.isDarwin then system else "aarch64-darwin";

  server = import (pkgs.path + "/nixos/lib/eval-config.nix") {
    system = linuxSystem;
    specialArgs.configurationMarker = "from the configuration";
    modules = [
      (
        { pkgs, configurationMarker, ... }:
        {
          nixpkgs.overlays = [
            (_: prev: { configurationMarker = prev.writeText "marker" configurationMarker; })
          ];
          environment.etc."configuration-marker".source = pkgs.configurationMarker;
          fileSystems."/" = {
            device = "/dev/vda";
            fsType = "ext4";
          };
          boot.loader.grub.devices = [ "nodev" ];
          users.users.root.openssh.authorizedKeys.keys = [ "ssh-ed25519 test" ];
          system.stateVersion = "25.05";
        }
      )
    ];
  };

  home = inputs.home-manager.lib.homeManagerConfiguration {
    inherit pkgs;
    extraSpecialArgs.configurationMarker = "from the home configuration";
    modules = [
      ({ configurationMarker, ... }: {
        home = {
          username = "jdoe";
          homeDirectory = if pkgs.stdenv.hostPlatform.isDarwin then "/Users/jdoe" else "/home/jdoe";
          stateVersion = "25.05";
          file."configuration-marker".text = configurationMarker;
        };
      })
    ];
  };

  darwin = inputs.nix-darwin.lib.darwinSystem {
    system = darwinSystem;
    specialArgs.configurationMarker = "from the Darwin configuration";
    modules = [
      ({ configurationMarker, ... }: {
        system.stateVersion = 6;
        nix.enable = false;
        nixpkgs.pkgs = import pkgs.path { system = darwinSystem; };
        environment.etc."configuration-marker".text = configurationMarker;
      })
    ];
  };
in
{
  machines.server = {
    target.host = "root@server.example.com";
    nixos = server;
  };

  machines.home = {
    home-manager = home;
  };

  machines.mac = {
    target.host = "admin@mac.example.com";
    nix-darwin = darwin;
  };

  outputs.darwinMarker =
    pkgs.writeText "darwin-marker"
      darwin.config.environment.etc."configuration-marker".text;
}
