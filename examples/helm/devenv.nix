{ pkgs, ... }:

{
  languages.helm = {
    enable = true;
    plugins = [ "helm-unittest" ];
  };
}
