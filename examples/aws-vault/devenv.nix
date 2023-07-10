{ pkgs, ... }: {
  languages.terraform.enable = true;

  aws-vault = {
    enable = true;
    profile = "aws-profile";
    terraformWrapperEnable = true;
  };
}
