{ pkgs, ... }: {
  languages.terraform.enable = true;

  aws-vault = {
    enable = true;
    profile = "aws-profile";
    awscli2WrapperEnable = true;
    terraformWrapperEnable = true;
  };
}
