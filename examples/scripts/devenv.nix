{ pkgs, ... }:

{
  scripts."gitversion".exec = ''
    echo hello $(${pkgs.git}/bin/git --version)
  '';
}
