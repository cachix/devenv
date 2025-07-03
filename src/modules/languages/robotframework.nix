{ pkgs, config, lib, ... }:

let
  cfg = config.languages.robotframework;
in
{
  options.languages.robotframework = {
    enable = lib.mkEnableOption "tools for Robot Framework development";

    python = lib.mkOption {
      type = lib.types.package;
      default = pkgs.python3;
      defaultText = lib.literalExpression "pkgs.python3";
      description = "The Python package to use.";
    };

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable Robot Framework development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable Robot Framework language server.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.python3Packages.robotframework-lsp;
          defaultText = lib.literalExpression "pkgs.python3Packages.robotframework-lsp";
          description = "The robotframework-lsp package to use.";
        };
      };
    };

  };

  config = lib.mkIf cfg.enable {
    packages = [
      (cfg.python.withPackages (ps: [
        ps.robotframework
      ]))
    ] ++ lib.optionals cfg.dev.enable (
      lib.optional cfg.dev.lsp.enable cfg.dev.lsp.package
    );
  };
}
