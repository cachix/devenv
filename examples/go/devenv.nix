{ pkgs, lib, config, inputs, ... }:

{
  packages = [ pkgs.git pkgs.gomod2nix ];

  languages.go.enable = true;
  languages.go.version = "1.25.4";

  git-hooks.hooks = {
    govet = {
      enable = true;
      pass_filenames = false;
    };
    gotest.enable = true;
    golangci-lint = {
      enable = true;
      pass_filenames = false;
    };
  };

  outputs =
    let
      name = "my-app";
      version = "1.0.0";
    in
    { app = import ./default.nix { inherit pkgs name version; }; };

  # See full reference at https://devenv.sh/reference/options/
}
