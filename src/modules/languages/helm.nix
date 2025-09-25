{ pkgs, config, lib, ... }:

let
  cfg = config.languages.helm;

  # Resolve plugin names to packages
  resolvedPlugins = builtins.map
    (name:
      pkgs.kubernetes-helmPlugins.${name} or
        (throw "Unknown Helm plugin: ${name}")
    )
    cfg.plugins;

  # https://github.com/NixOS/nixpkgs/issues/217768
  helm-plugins-dir = pkgs.symlinkJoin {
    name = "helm-plugins";
    paths = resolvedPlugins;
  };
in
{
  options.languages.helm = {
    enable = lib.mkEnableOption "tools for Helm development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.kubernetes-helm;
      defaultText = lib.literalExpression "pkgs.kubernetes-helm";
      description = "The Helm package to use.";
    };

    plugins = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      example = [ "helm-secrets" "helm-diff" "helm-unittest" ];
      description = ''
        List of Helm plugin names to include from pkgs.kubernetes-helmPlugins.

        They will be symlinked into one directory and exposed via HELM_PLUGINS.
      '';
    };

    languageServer = {
      enable = lib.mkEnableOption "Helm language server";

      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.helm-ls;
        defaultText = lib.literalExpression "pkgs.helm-ls";
        description = "The Helm language server package to include.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages =
      [ cfg.package ]
      ++ lib.optional cfg.languageServer.enable cfg.languageServer.package;

    env.HELM_PLUGINS = "${helm-plugins-dir}";
  };
}

