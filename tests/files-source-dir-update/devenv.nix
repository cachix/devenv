{ pkgs, ... }:
{
  files."src-dir".source = pkgs.writeTextDir "foo.txt" "old";
}
