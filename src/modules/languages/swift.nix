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
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
      pkgs.clang
    ];

    env.CC = "${pkgs.clang}/bin/clang";
  };
}
