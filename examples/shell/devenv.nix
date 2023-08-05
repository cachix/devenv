{ pkgs, ... }:

{
  languages.shell.enable = true;
  enterShell = ''
    bash-language-server --version
    bats --version
    shellcheck --version
    shfmt --version
  '';
}
