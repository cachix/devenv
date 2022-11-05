{ pkgs, ... }:

{
  packages = [
    (import ./src/devenv.nix { inherit pkgs; })
    pkgs.python3Packages.mkdocs-material
  ];

  processes.docs.exec = "mkdocs serve";

  enterShell = ''
    echo hola
  '';

  scripts."run-devenv-tests".exec = ''
    set -xe

    for dir in $(ls examples); do
      pushd examples/$dir 
      devenv ci
      devenv shell
      exit
      popd
    done

    # TODO: test direnv integration
    # TODO: test DIRENV_ACTIVE
  '';

  pre-commit.hooks = {
    nixpkgs-fmt.enable = true;
    shellcheck.enable = true;
    markdownlint.enable = true;
  };
}
