{ pkgs, config, lib, ... }:

let
  cfg = config.languages.pascal;
in
{
  options.languages.pascal = {
    enable = lib.mkEnableOption "tools for Pascal development";

    lazarus = {
      enable = lib.mkEnableOption "lazarus graphical IDE for the FreePascal language";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      fpc
    ] ++ lib.optional (cfg.lazarus.enable && pkgs.stdenv.isLinux) pkgs.lazarus;
  };
}
