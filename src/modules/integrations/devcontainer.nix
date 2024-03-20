{ pkgs, lib, config, ... }:

let
  cfg = config.devcontainer;
  settingsFormat = pkgs.formats.json { };
  file = settingsFormat.generate "devcontainer.json" cfg.settings;
in
{
  options.devcontainer = {
    enable = lib.mkEnableOption "generation .devcontainer.json for devenv integration";

    settings = lib.mkOption {
      type = lib.types.submodule {
        freeformType = settingsFormat.type;

        options.image = lib.mkOption {
          type = lib.types.str;
          default = "ghcr.io/cachix/devenv:latest";
          description = lib.mdDoc ''
            The name of an image in a container registry.
          '';
        };

        options.overrideCommand = lib.mkOption {
          type = lib.types.anything;
          default = false;
          description = lib.mdDoc ''
            Override the default command.
          '';
        };

        options.updateContentCommand = lib.mkOption {
          type = lib.types.anything;
          default = "devenv test";
          description = lib.mdDoc ''
            Command to run after container creation.
          '';
        };

        options.customizations.vscode.extensions = lib.mkOption {
          type = lib.types.listOf lib.types.str;
          default = [ "mkhl.direnv" ];
          description = lib.mdDoc ''
            List of preinstalled VSCode extensions.
          '';
        };
      };

      default = { };

      description = lib.mdDoc ''
        Devcontainer settings.
      '';
    };
  };

  config = lib.mkIf config.devcontainer.enable {
    enterShell = ''
      cat ${file} > ${config.env.DEVENV_ROOT}/.devcontainer.json
    '';
  };
}
