{ pkgs, ... }:

{
  users.users.dev.shell = pkgs.bashInteractive;

  devenvTest.rcLines.".bashrc" = [
    "# bash defaults"
    "export PS1='$ '"
  ];

  devenvTest.rcLines.".bash_profile" = [
    "[ -f ~/.bashrc ] && . ~/.bashrc"
  ];
}
