{ pkgs, lib, config, ... }:

let
  cfg = config.devcontainer;
in
{
  options.devcontainer = {
    enable = lib.mkEnableOption "generation .devcontainer.json for devenv integration";

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

  config = lib.mkIf config.devcontainer.enable {
    files.".devcontainer.json".json = cfg.settings;
  };
}
