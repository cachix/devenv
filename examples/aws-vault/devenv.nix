{ pkgs, ... }: {
  languages.terraform.enable = true;

  aws-vault = {
    enable = true;
    profile = "aws-profile";
    awscli2Wrapper.enable = true;
    terraformWrapper.enable = true;
  };
}
