{ pkgs, config, lib, ... }:

let
  cfg = config.languages.terraform;
in
{
  options.languages.terraform = {
    enable = lib.mkEnableOption "tools for terraform development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.terraform;
      defaultText = lib.literalExpression "pkgs.terraform";
      description = "The terraform package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      cfg.package
      terraform-ls
      tfsec
    ];
  };
}
