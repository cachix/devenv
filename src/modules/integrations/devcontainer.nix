{ pkgs, lib, config, ... }:

let
  cfg = config.devcontainer;
in
{
  options.devcontainer = {
    enable = lib.mkEnableOption "generation .devcontainer.json for devenv integration";

    copyMode = mkOption {
        type = types.enum [ "seed" "copy" ];
        default = "copy";
        description = ''
          Difference between options

          - `seed`: copy the file into place once, only if it does not already exist, and make it writable.
          - `copy`: copy the file into place as a writable file, overwriting it with fresh contents on every shell entry.
        '';
      };

    settings = lib.mkOption {
      type = lib.types.submodule {
        freeformType = (pkgs.formats.json { }).type;

        options.image = lib.mkOption {
          type = lib.types.str;
          default = "ghcr.io/cachix/devenv/devcontainer:latest";
          description = ''
            The name of an image in a container registry.
          '';
        };

        options.overrideCommand = lib.mkOption {
          type = lib.types.anything;
          default = false;
          description = ''
            Override the default command.
          '';
        };

        options.updateContentCommand = lib.mkOption {
          type = lib.types.anything;
          default = "devenv test";
          description = ''
            A command to run after the container is created.
          '';
        };

        options.customizations.vscode.extensions = lib.mkOption {
          type = lib.types.listOf lib.types.str;
          default = [ "mkhl.direnv" ];
          description = ''
            A list of pre-installed VS Code extensions.
          '';
        };
      };

      default = { };

      description = ''
        Devcontainer settings.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    files.".devcontainer/devcontainer.json" = { copyMode = cfg.copyMode; json = cfg.settings; };
  };
}
