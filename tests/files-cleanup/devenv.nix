{ pkgs, ... }: {
  files."a.txt".text = "a";
  files."b.txt".text = "b";
  files."subdir/nested.txt".text = "nested";
}
