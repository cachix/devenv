{ pkgs, lib, ... }:

{
  packages = [
    pkgs.diffutils
  ];

  git-hooks.hooks = {
    treefmt.enable = true;
  };

  treefmt = {
    enable = true;

    config.programs = {
      nixfmt.enable = true;
      rustfmt.enable = true;
    };

    config.settings.formatter = {
      "yq-json" = {
        command = "${lib.getExe pkgs.bash}";
        options = [
          "-euc"
          ''
            for file in "$@"; do
              ${lib.getExe pkgs.yq-go} -i --output-format=json $file
            done
          ''
          "--" # bash swallows the second argument when using -c
        ];
        includes = [ "*.json" ];
        excludes = [
          ".git/*"
          ".devenv/*"
        ];
      };
    };
  };
}
