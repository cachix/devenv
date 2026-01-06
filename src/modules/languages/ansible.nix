{ pkgs, config, lib, ... }:

let
  cfg = config.languages.ansible;
  # ansible-language-server may not be available in all nixpkgs versions
  hasLsp = pkgs ? ansible-language-server && (builtins.tryEval pkgs.ansible-language-server).success;
in
{
  options.languages.ansible = {
    enable = lib.mkEnableOption "tools for Ansible development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.ansible;
      defaultText = lib.literalExpression "pkgs.ansible";
      description = "The Ansible package to use.";
    };

    lsp = {
      enable = lib.mkEnableOption "Ansible Language Server" // {
        default = hasLsp;
      };

      package = lib.mkOption {
        type = lib.types.nullOr lib.types.package;
        default = if hasLsp then pkgs.ansible-language-server else null;
        defaultText = lib.literalExpression "pkgs.ansible-language-server";
        description = "The Ansible language server package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      pkgs.ansible-lint
      cfg.package
    ] ++ lib.optional (cfg.lsp.enable && cfg.lsp.package != null) cfg.lsp.package;
  };
}

