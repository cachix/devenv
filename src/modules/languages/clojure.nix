{ pkgs, config, lib, ... }:

let
  cfg = config.languages.clojure;
in
{
  options.languages.clojure = {
    enable = lib.mkEnableOption "Enable tools for Clojure development.";
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      clojure
      clojure-lsp
    ];
  };
}
