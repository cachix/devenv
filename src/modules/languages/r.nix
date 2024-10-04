{ pkgs, config, lib, ... }:

let
  cfg = config.languages.r;
in
{
  options.languages.r = {
    enable = lib.mkEnableOption "tools for R development";
    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.R;
      defaultText = lib.literalExpression "pkgs.R";
      description = "The R package to use.";
    };
    radian = {
      enable = lib.mkEnableOption "a 21 century R console";
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.radianWrapper;
        defaultText = lib.literalExpression "pkgs.radianWrapper";
        description = "The radian package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [ cfg.package ] ++ lib.lists.optional cfg.radian.enable cfg.radian.package;
  };
}
