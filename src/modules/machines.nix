{ pkgs, config, lib, self, ... }:

let
  machineOptions = lib.types.submodule ({ name, config, ... }: {
    options = {
      system = lib.mkOption {
        type = lib.types.str;
        description = "System architecture for the machine.";
        default = pkgs.stdenv.system;
        defaultText = lib.literalExpression "pkgs.stdenv.system";
        example = "x86_64-linux";
      };

      nixos = lib.mkOption {
        type = lib.types.nullOr lib.types.unspecified;
        description = "NixOS configuration for the machine.";
        default = null;
        example = lib.literalExpression ''
          {
            fileSystems."/".device = "/dev/sda1";
            boot.loader.systemd-boot.enable = true;
            services.openssh.enable = true;
          }
        '';
      };

      home-manager = lib.mkOption {
        type = lib.types.nullOr lib.types.unspecified;
        description = "Home Manager configuration for the machine.";
        default = null;
        example = lib.literalExpression ''
          {
            home.username = "jdoe";
            home.homeDirectory = "/home/jdoe";
            programs.git.enable = true;
          }
        '';
      };

      nix-darwin = lib.mkOption {
        type = lib.types.nullOr lib.types.unspecified;
        description = "nix-darwin configuration for the machine.";
        default = null;
        example = lib.literalExpression ''
          { pkgs, ... }: {
            environment.systemPackages = [
              pkgs.vim
            ];
            services.nix-daemon.enable = true;
          }
        '';
      };
    };
  });
in
{
  imports = [
    (lib.mkRenamedOptionModule [ "configurations" ] [ "machines" ])
  ];

  options = {
    machines = lib.mkOption {
      type = lib.types.attrsOf machineOptions;
      default = { };
      description = "Machines for NixOS, home-manager, and nix-darwin.";
    };
  };
}
