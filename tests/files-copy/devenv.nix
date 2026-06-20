{ pkgs, ... }: {
  files."template.txt" = {
    text = "default content\n";
    copy = "copy";
  };
  files."managed.txt" = {
    text = "managed content\n";
    copy = "replace";
  };
}
