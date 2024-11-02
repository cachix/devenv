{ pkgs, ... }:

{
  # https://nixos.org/manual/nixpkgs/stable/#sec-language-texlive
  languages.texlive = {
    enable = true;

    # Choose a base package set.
    base = pkgs.texliveSmall;

    # Add extra packages to the base set.
    packages = [ "latexmk" ];
  };
}
