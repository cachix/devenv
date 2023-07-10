{ pkgs, ... }: {
  aws-vault = {
    enable = true;
    profile = "aws-profile";
    terraformWrapperEnable = true;
  };
}
