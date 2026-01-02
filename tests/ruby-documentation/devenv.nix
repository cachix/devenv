{ pkgs, ... }:
{
  languages.ruby = {
    enable = true;
    version = "3.4.7";
    documentation.enable = true;
  };

  enterTest = ''
    ri Object >/dev/null
  '';

}
