{ pkgs, ... }:
{
  packages = [ pkgs.git ];

  git.config = {
    merge.conflictStyle = "diff3";
    merge.tool = "vimdiff";
    merge.test = {
      name = "Test merge driver";
      driver = "cp %B %A";
    };
    test = {
      boolean = true;
      number = 42;
      quoted = "spaces, \"quotes\", and \\backslashes";
      repeated = [
        "first"
        "second"
      ];
    };
  };

  git.attributes = [
    "*.txt merge=test"
    "*.dat binary"
  ];
}
