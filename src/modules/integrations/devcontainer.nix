{ pkgs, lib, config, ... }:

let
  file = pkgs.writeText "devcontainer.json" ''
    {
      "image": "ghcr.io/cachix/devenv:latest",
      "overrideCommand": false,
      "customizations": {
        "vscode": {
          "extensions": ["mkhl.direnv"]
        }
      }
    }
  '';
in
{
  options.devcontainer = {
    enable = lib.mkEnableOption "Generate .devcontainer.json for devenv integration.";
  };

  config = lib.mkIf config.devcontainer.enable {
    enterShell = ''
      cat ${file} > ${config.env.DEVENV_ROOT}/.devcontainer.json
    '';
  };
}
