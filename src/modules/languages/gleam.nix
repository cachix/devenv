{ pkgs, config, lib, inputs, ... }:

let
  cfg = config.languages.gleam;

  setup = ''
    To use gleam, you need to add the following to your devenv.yaml:

      inputs:
         gleam-nix:
           url: github:vic/gleam-nix
           overlays:
             - default


    Optionally, if you want a specific gleam branch or version, do
    the following:

      inputs:
        gleam:
           url: github:gleam-lang/gleam/main # or any other branch
           flake: false
        gleam-nix:
           url: github:vic/gleam-nix
           overlays:
             - default
           inputs:
             gleam = "gleam"
  '';

  gleamPkg = pkgs.gleam or (throw setup);

in
{
  options.languages.gleam = {
    enable = lib.mkEnableOption "tools for Gleam development";

    package = lib.mkOption {
      type = lib.types.package;
      default = gleamPkg;
      description = "The Gleam package to use.";
      defaultText = lib.literalExpression "pkgs.gleam";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ];
  };
}
