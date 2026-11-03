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

  standalone = import (pkgs.path + "/nixos/lib/eval-config.nix") {
    system = linuxSystem;
    modules = [
      inputs.devenv.nixosModules.machines
      {
        devenv.secrets.TOKEN = { };
        fileSystems."/" = {
          device = "/dev/vda";
          fsType = "ext4";
        };
        boot.loader.grub.devices = [ "nodev" ];
        system.stateVersion = "25.05";
      }
    ];
  };

  skewed = import (pkgs.path + "/nixos/lib/eval-config.nix") {
    system = linuxSystem;
    modules = [
      ./older-machines.nix
      {
        fileSystems."/" = {
          device = "/dev/vda";
          fsType = "ext4";
        };
        boot.loader.grub.devices = [ "nodev" ];
        system.stateVersion = "25.05";
      }
    ];
  };

  compatible = server.extendModules {
    modules = [
      (import ./compatible-machines.nix { inherit inputs; })
      { devenv.secrets.TOKEN = { }; }
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

  machines.standalone = {
    target.host = "root@standalone.example.com";
    nixos = standalone;
  };

  machines.skewed = {
    target.host = "root@skewed.example.com";
    nixos = skewed;
  };
  machines.compatible = {
    target.host = "root@compatible.example.com";
    nixos = compatible;
  };
  outputs.darwinMarker =
    pkgs.writeText "darwin-marker"
      darwin.config.environment.etc."configuration-marker".text;
  outputs.standalone = standalone.config.system.build.toplevel;
}
