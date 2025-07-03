{ pkgs, config, lib, ... }:

let
  cfg = config.languages.swift;
in
{
  options.languages.swift = {
    enable = lib.mkEnableOption "tools for Swift development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.swift;
      defaultText = lib.literalExpression "pkgs.swift";
      description = ''
        The Swift package to use.
      '';
    };

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable Swift development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable sourcekit-lsp language server.";
        };
      };

      formatter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable swift-format formatter.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.swift-format;
          defaultText = lib.literalExpression "pkgs.swift-format";
          description = "The swift-format package to use.";
        };
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
      pkgs.clang
    ] ++ lib.optionals cfg.dev.enable (
      lib.optional cfg.dev.formatter.enable cfg.dev.formatter.package
    );

    env.CC = "${pkgs.clang}/bin/clang";
  };
}
