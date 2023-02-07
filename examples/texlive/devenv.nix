{ pkgs, ... }:

{
  languages.texlive = {
    enable = true;
    packages = [ "scheme-small" "biblatex" "latexmk" ];
  };
}
