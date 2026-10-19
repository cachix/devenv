{ pkgs, ... }:
{
  packages = [ pkgs.git ];
  git.attributes = [ "*.dat binary" ];
}
