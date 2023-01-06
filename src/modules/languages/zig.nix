{ pkgs, config, lib, ... }:

let
  cfg = config.languages.zig;
in
{
  options.languages.zig = {
    enable = lib.mkEnableOption "Enable tools for Zig development.";

    package = lib.mkOption {
      type = lib.types.package;
      description = "Which package of Zig to use";
      default = pkgs.zig;
      defaultText = "pkgs.zig";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ];
  };
}
