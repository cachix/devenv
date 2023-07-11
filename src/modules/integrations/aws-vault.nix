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

    awscli2Wrapper = lib.mkOption {
      type = lib.types.submodule {
        options = {
          enable = lib.mkEnableOption ''
            Wraps awscli2 binary as `aws-vault exec <profile> -- aws <args>`.
          '';

          package = lib.mkOption {
            type = lib.types.package;
            default = pkgs.awscli2;
            defaultText = lib.literalExpression "pkgs.awscli2";
            description = "The awscli2 package to use.";
          };
        };
      };
      defaultText = lib.literalExpression "pkgs";
      default = { };
      description = "Attribute set of packages including awscli2";
    };

    terraformWrapper = lib.mkOption {
      type = lib.types.submodule {
        options = {
          enable = lib.mkEnableOption ''
            Wraps terraform binary as `aws-vault exec <profile> -- terraform <args>`.
          '';

          package = lib.mkOption {
            type = lib.types.package;
            default = pkgs.terraform;
            defaultText = lib.literalExpression "pkgs.terraform";
            description = "The terraform package to use.";
          };
        };
      };
      defaultText = lib.literalExpression "pkgs";
      default = { };
      description = "Attribute set of packages including terraform";
    };
  };

  config = lib.mkMerge [
    (lib.mkIf (cfg.enable && cfg.awscli2Wrapper.enable) {
      packages = [
        (pkgs.writeScriptBin "aws" ''
          ${cfg.package}/bin/aws-vault exec ${cfg.profile} -- ${cfg.awscli2Wrapper.package}/bin/aws "$@"
        '')
      ];
    })
    (lib.mkIf (cfg.enable && cfg.terraformWrapper.enable) {
      languages.terraform.package = pkgs.writeScriptBin "terraform" ''
        ${cfg.package}/bin/aws-vault exec ${cfg.profile} -- ${cfg.terraformWrapper.package}/bin/terraform "$@"
      '';
    })
  ];
}
