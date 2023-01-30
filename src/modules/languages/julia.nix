{ pkgs, config, lib, ... }:

let
  cfg = config.languages.julia;
in
{
  options.languages.julia = {
    enable = lib.mkEnableOption "tools for Julia development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.julia-bin;
      defaultText = lib.literalExpression "pkgs.julia-bin";
      description = "The Julia package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ];
  };
}
