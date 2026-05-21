{ pkgs, ... }:

{
  programs.zsh.enable = true;
  users.users.dev.shell = pkgs.zsh;

  devenvTest.rcLines.".zshrc" = [
    "# zsh defaults"
    "export PS1='%% '"
  ];
}
