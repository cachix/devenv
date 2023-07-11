{ pkgs, ... }: {
  languages.terraform.enable = true;

  aws-vault = {
    enable = true;
    profile = "aws-profile";
    awscliWrapper.enable = true;
    terraformWrapper.enable = true;
  };
}
