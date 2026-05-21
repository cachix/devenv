{ ... }:

{
  imports = [
    ../fragments/shells/zsh.nix
    ../fragments/tools/zoxide.nix
  ];

  devenvTest.rcLines.".zshrc" = [
    ''alias cd="z"''
  ];
}
