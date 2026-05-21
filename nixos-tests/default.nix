{ pkgs ? import <nixpkgs> { }
, devenv ? null
}:

let
  lib = pkgs.lib;
  testLib = import ./lib.nix { inherit pkgs lib devenv; };
  inherit (testLib) mkFixture mkTest buildMatrix;

  minimalProject = {
    devenvYaml = ./projects/minimal/devenv.yaml;
    devenvNix = ./projects/minimal/devenv.nix;
  };

  fixtures = [
    (mkFixture {
      name = "bash-plain";
      caps = { shell = "bash"; };
      module = import ./fixtures/bash-plain.nix;
    })

    (mkFixture {
      name = "zsh-zoxide-aliased";
      caps = { shell = "zsh"; tools = [ "zoxide" ]; };
      module = import ./fixtures/zsh-zoxide-aliased.nix;
    })

    (mkFixture {
      name = "bash-plain-with-project";
      caps = { shell = "bash"; };
      module = import ./fixtures/bash-plain.nix;
      project = minimalProject;
    })

    (mkFixture {
      name = "zsh-zoxide-aliased-with-project";
      caps = { shell = "zsh"; tools = [ "zoxide" ]; };
      module = import ./fixtures/zsh-zoxide-aliased.nix;
      project = minimalProject;
    })
  ];

  tests = [
    (mkTest (import ./tests/shell-startup-clean.nix))
  ]
  ++ lib.optional (devenv != null)
    (mkTest (import ./tests/devenv-version.nix))
  ++ lib.optional (devenv != null)
    (mkTest (import ./tests/devenv-shell-enter.nix));

in
buildMatrix { inherit fixtures tests; }
