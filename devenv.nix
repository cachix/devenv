{ pkgs, ... }: 

{
  packages = [ (import ./src/devenv.nix { inherit pkgs; }) ];

  enterShell = ''
    echo hola
  '';
}