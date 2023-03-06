{ pkgs, config, lib, ... }:

let
  cfg = config.languages.cplusplus;
in
{
  options.languages.cplusplus = {
    enable = lib.mkEnableOption "tools for C++ development";
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      cmake
      clang
      ccls
    ];
  };
}
