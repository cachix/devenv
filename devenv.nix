{ pkgs, ... }:

{
  packages = [ (import ./src/devenv.nix { inherit pkgs; }) ];

  processes.whileloop.exec = "while true; do echo waiting; sleep 1; done";

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
  '';

  pre-commit.hooks = {
    nixpkgs-fmt.enable = true;
    shellcheck.enable = true;
  };
}
