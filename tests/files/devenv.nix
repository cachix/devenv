{
  pkgs,
  lib,
  config,
  ...
}:
{

  packages = [ pkgs.yq-go ];

  files = {
    "foo.json".json = {
      foo = "bar";
    };
    "foo.ini".ini = {
      foo = {
        bar = "baz";
      };
    };
    "foo.yaml".yaml = {
      foo = "bar";
    };
    "foo.toml".toml = {
      foo = "bar";
    };
    "foo.txt".text = "foo";

    "dir/foo.txt".text = "foo";

    "script.sh" = {
      text = "#!/bin/bash\necho hello";
      executable = true;
    };

    "src.txt".source = pkgs.writeText "src.txt" "foo";
    "src-dir".source = pkgs.writeTextDir "foo.txt" "foo";
  };
}
