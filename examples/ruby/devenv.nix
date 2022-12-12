{ pkgs, ... }:

{
  languages.ruby.enable = true;

  enterShell = ''
    bundle
  '';
}
