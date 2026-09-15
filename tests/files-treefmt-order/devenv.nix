{ lib, ... }:
{
  treefmt.enable = true;
  treefmt.config.programs.nixfmt.enable = true;

  files."managed.txt" = {
    text = "managed content\n";
    copyMode = "copy";
  };

  # Use a slow producer and a reader to exercise the integration's ordering edge.
  tasks."devenv:files".exec = lib.mkForce ''
    sleep 1
    printf 'managed content\n' > managed.txt
  '';
  tasks."devenv:treefmt:run".exec = lib.mkForce ''
    grep -qx "managed content" managed.txt
  '';
}
