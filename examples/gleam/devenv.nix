{ pkgs, ... }:

{
  # https://devenv.sh/languages/
  languages.gleam.enable = true;

  enterShell = ''
    gleam --version
  '';

  # See full reference at https://devenv.sh/reference/options/
}
