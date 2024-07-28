{ pkgs, config, lib, ... }:

let
  cfg = config.languages.clojure;
in
{
  options.languages.clojure = {
    enable = lib.mkEnableOption "tools for Clojure development";
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      (clojure.override {
        jdk = config.languages.java.jdk.package;
      })
      clojure-lsp
    ];
    languages.java.enable = true;
  };
}
