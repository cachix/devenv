{ pkgs, config, lib, ... }:

let
  cfg = config.languages.ansible;
in
{
  options.languages.ansible = {
    enable = lib.mkEnableOption "tools for Ansible development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.ansible;
      defaultText = lib.literalExpression "pkgs.ansible";
      description = "The Ansible package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      ansible-lint
      cfg.package
    ];
  };
}

