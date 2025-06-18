{ pkgs, ... }:

{
  languages.r = {
    enable = true;
    radian = {
      enable = true;
      package = pkgs.python312Packages.radian;
    };
    descriptionFile = {
      path = "${./DESCRIPTION}";
      installPackages.enable = true;
    };
  };
}
