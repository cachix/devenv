let
  nixpkgs = fetchTarball "https://github.com/cachix/devenv-nixpkgs/archive/rolling.tar.gz";
  pkgs = import nixpkgs { };
  devenv-src = fetchTarball "https://github.com/cachix/devenv/archive/main.tar.gz";
  devenv = (import devenv-src).lib.mkStandardShell ./.;
in
{
  shell = devenv {
    inherit pkgs;
    modules = [ ./devenv.nix ];
  };
}
