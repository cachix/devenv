{ pkgs, config, lib, ... }:

let
  cfg = config.languages.ansible;
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

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable Ansible development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable ansible-language-server language server.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.ansible-language-server;
          defaultText = lib.literalExpression "pkgs.ansible-language-server";
          description = "The ansible-language-server package to use.";
        };
      };

      linter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable ansible-lint linter.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.ansible-lint;
          defaultText = lib.literalExpression "pkgs.ansible-lint";
          description = "The ansible-lint package to use.";
        };
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ] ++ lib.optionals cfg.dev.enable (
      lib.optional (cfg.dev.lsp.enable) cfg.dev.lsp.package ++
        lib.optional (cfg.dev.linter.enable) cfg.dev.linter.package
    );
  };
}

