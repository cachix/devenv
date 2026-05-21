{ pkgs, ... }:

{
  environment.systemPackages = [ pkgs.zoxide ];

  devenvTest.rcLines.".zshrc" = [
    ''eval "$(zoxide init zsh)"''
  ];

  devenvTest.rcLines.".bashrc" = [
    ''eval "$(zoxide init bash)"''
  ];
}
