{ pkgs, config, lib, ... }:

let
  cfg = config.languages.clojure;
in
{
  options.languages.clojure = {
    enable = lib.mkEnableOption "tools for Clojure development";

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable Clojure development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable clojure-lsp language server.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.clojure-lsp;
          defaultText = lib.literalExpression "pkgs.clojure-lsp";
          description = "The clojure-lsp package to use.";
        };
      };

      formatter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable cljfmt formatter.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.clojure-lsp;
          defaultText = lib.literalExpression "pkgs.clojure-lsp";
          description = "The cljfmt package to use.";
        };
      };

      linter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable clj-kondo linter.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.clj-kondo;
          defaultText = lib.literalExpression "pkgs.clj-kondo";
          description = "The clj-kondo package to use.";
        };
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      (pkgs.clojure.override {
        jdk = config.languages.java.jdk.package;
      })
    ] ++ lib.optionals cfg.dev.enable (
      lib.optional (cfg.dev.lsp.enable) cfg.dev.lsp.package ++
        lib.optional (cfg.dev.formatter.enable) cfg.dev.formatter.package ++
        lib.optional (cfg.dev.linter.enable) cfg.dev.linter.package
    );
    languages.java.enable = true;
  };
}
