{ pkgs, lib, ... }:

{
  formatting = {
    enable = true;

    treefmt.projectRootFile = "projectRootFile";

    treefmt.programs = {
      nixpkgs-fmt.enable = true;
      nixfmt.enable = true;
      rustfmt.enable = true;
    };
  };

  git-hooks.hooks = {
    treefmt.enable = true;
  };

  formatting.treefmt.settings.formatter = {
    "yq-json" = {
      command = "${pkgs.bash}/bin/bash";
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
      excludes = [ ".git/*" ".devenv/*" ];
    };
  };
}
