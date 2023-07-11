{ pkgs, config, lib, ... }:

let
  cfg = config.aws-vault;
in
{
  options.aws-vault = {
    enable = lib.mkEnableOption "aws-vault integration";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.aws-vault;
      defaultText = lib.literalExpression "pkgs.aws-vault";
      description = "The aws-vault package to use.";
    };

    profile = lib.mkOption {
      type = lib.types.str;
      description = lib.mdDoc ''
        The profile name passed to `aws-vault exec`.
      '';
    };

    awscli2WrapperEnable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Wraps awscli2 binary as `aws-vault exec <profile> -- aws <args>`.
      '';
    };

    terraformWrapperEnable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Wraps terraform binary as `aws-vault exec <profile> -- terraform <args>`.
      '';
    };
  };

  config = lib.mkMerge [
    (lib.mkIf (cfg.enable && cfg.awscli2WrapperEnable) {
      packages = [
        (pkgs.writeScriptBin "aws" ''
          ${cfg.package}/bin/aws-vault exec ${cfg.profile} -- ${pkgs.awscli2}/bin/aws "$@"
        '')
      ];
    })
    (lib.mkIf (cfg.enable && cfg.terraformWrapperEnable) {
      languages.terraform.package = pkgs.writeScriptBin "terraform" ''
        ${cfg.package}/bin/aws-vault exec ${cfg.profile} -- ${pkgs.terraform}/bin/terraform "$@"
      '';
    })
  ];
}
