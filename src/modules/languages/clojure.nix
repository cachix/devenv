{ pkgs, config, lib, ... }:

let
  cfg = config.languages.clojure;
in
{
  options.languages.clojure = {
    enable = lib.mkEnableOption "tools for Clojure development";
    leiningen = {
      enable = lib.mkEnableOption "leiningen";
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.leiningen;
        defaultText = lib.literalExpression "pkgs.leiningen";
        description = "The leiningen package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      (cfg.leiningen.enable && cfg.leiningen.package)
      (clojure.override {
        jdk = config.languages.java.jdk.package;
      })
      clojure-lsp
    ];
    languages.java.enable = true;
  };
}
