{ pkgs, ... }:
{
  languages.ruby = {
    enable = true;
    version = "3.4.7";
    docSupport = true;
  };

  enterTest = ''
    ri Object >/dev/null
  '';

}
