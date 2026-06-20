{ pkgs, ... }: {
  files."template.txt" = {
    text = "default content\n";
    copyMode = "seed";
  };
  files."managed.txt" = {
    text = "managed content\n";
    copyMode = "copy";
  };
}
