{ pkgs, config, lib, ... }:

let
  cfg = config.languages.clojure;
in
{
  options.languages.clojure = {
    enable = lib.mkEnableOption "tools for Clojure development";

    lsp = {
      enable = lib.mkEnableOption "Clojure Language Server" // { default = true; };

      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.clojure-lsp;
        defaultText = lib.literalExpression "pkgs.clojure-lsp";
        description = "The Clojure language server package to use.";
      };
    };

    jdk = {
      package = lib.mkOption {
        type = lib.types.nullOr lib.types.package;
        default = null;
        description = "The JDK package to use for Clojure. When `null`, the default JDK bundled with `pkgs.clojure` is used.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      (
        if cfg.jdk.package == null then
          pkgs.clojure
        else
          pkgs.clojure.override { jdk = cfg.jdk.package; }
      )
    ] ++ lib.optional cfg.lsp.enable cfg.lsp.package;
  };
}
