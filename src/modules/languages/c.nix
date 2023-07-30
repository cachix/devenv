{ pkgs, config, lib, ... }:

let
  cfg = config.languages.c;

  # Default to GCC if no compiler is explicitly enabled
  useGcc = cfg.gcc.enable || !cfg.clang.enable;
  useClang = cfg.clang.enable;
in
{
  options.languages.c = {
    enable = lib.mkEnableOption "tools for C development";

    clang.enable = lib.mkEnableOption "clang";
    gcc.enable = lib.mkEnableOption "GCC";
  };

  config = lib.mkIf cfg.enable {
    languages.c = {
      clang.enable = lib.mkDefault false;
      gcc.enable = lib.mkDefault false;
    };

    packages = (with pkgs; [
      gnumake
      pkg-config

      ccls # LSP server
    ]) ++
    (lib.optional useClang pkgs.clang) ++
    (lib.optional useGcc pkgs.gcc);

    env =
      if useClang then {
        CC = "clang";
        CXX = "clang++";
      } else
        if useGcc then {
          CC = "gcc";
          CXX = "g++";
        } else { };
  };
}
