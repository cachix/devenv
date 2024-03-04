{ pkgs, ... }:

{
  # Since Terraform adopted a non-free license (BSL 1.1) in August 2023,
  # using terraform instead of opentofu now requires adding `allowUnfree: true` to `devenv.yaml`
  languages.opentofu.enable = true;

  aws-vault = {
    enable = true;
    profile = "aws-profile";
    awscliWrapper.enable = true;
    opentofuWrapper.enable = true;
  };
}
