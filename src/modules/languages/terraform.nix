{ pkgs, config, lib, ... }:

let
  cfg = config.languages.terraform;
in
{
  options.languages.terraform = {
    enable = lib.mkEnableOption "tools for Terraform development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.terraform;
      defaultText = lib.literalExpression "pkgs.terraform";
      description = "The Terraform package to use.";
    };

    awsProfile = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = lib.mdDoc ''
        Defines `AWS_PROFILE` environment variable if `enableAwsVaultWrapper`
        is set to `false`, otherwise, it becomes the name of the profile passed
        to `aws-vault exec`.
      '';
    };

    enableAwsVaultWrapper = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = lib.mdDoc ''
        Create a script that replaces the original Terraform binary with the
        following wrapper if enabled:

        ```sh
        aws-vault exec <profile> -- terraform "$@"
        ```

        Where `<profile>` is the value coming from `awsProfile`.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    env = lib.mkIf (cfg.awsProfile != null && !cfg.enableAwsVaultWrapper) {
      AWS_PROFILE = cfg.awsProfile;
    };

    packages = with pkgs; [
      terraform-ls
      tfsec
      (if cfg.enableAwsVaultWrapper then
        writeScriptBin "terraform" ''
          ${aws-vault}/bin/aws-vault exec ${cfg.awsProfile} -- \
            ${cfg.package}/bin/terraform "$@"
        ''
      else cfg.package)
    ];
  };
}
