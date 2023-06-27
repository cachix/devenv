{ pkgs, ... }:

{
  languages.jsonnet.enable = true;
  enterShell = ''
    jsonnet --version
  '';
}
