{ pkgs, ... }: 

{
  packages = [ (import ./src/devenv.nix { inherit pkgs; }) ];

  processes.whileloop.exec = "while true; do echo waiting; sleep 1; done";

  enterShell = ''
    echo hola
  '';
}