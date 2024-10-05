{ pkgs, ... }:

{
  languages.r = {
    enable = true;
    radian = {
      enable = true;
      package = pkgs.python312Packages.radian;
    };
  };
}
