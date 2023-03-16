{ pkgs, config, lib, ... }:

let
  cfg = config.languages.c;
in
{
  options.languages.c = {
    enable = lib.mkEnableOption "tools for C development";
    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.stdenv.cc;
      example = "pkgs.clangStdenv.cc";
      defaultText = "pkgs.stdenv.cc";
      description = "C/C++ toolchain to use. Defaults to nixpkgs default C compiler (i.e. gcc on Linux and clang on macOS)";
    };
    languageServer = lib.mkOption {
      type = lib.types.nullOr lib.types.package;
      default = pkgs.clang-tools;
      example = pkgs.ccls;
      defaultText = "pkgs.clang-tools";
      description = "Language server to use. Defaults to clangd from the clang-tools package.";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      gnumake
      pkg-config

      cfg.package
      cfg.languageServer
    ];
  };
}
