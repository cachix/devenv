{ pkgs, config, lib, ... }:

let
  cfg = config.languages.javascript;
in
{
  options.languages.javascript = {
    enable = lib.mkEnableOption "Enable tools for JavaScript development.";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.nodejs;
      defaultText = "pkgs.nodejs";
      description = "The Node package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      cfg.package
    ];
  };
}
