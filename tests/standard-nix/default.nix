let
  nixpkgs = fetchTarball "https://github.com/cachix/devenv-nixpkgs/archive/rolling.tar.gz";
  pkgs = import nixpkgs { };
  devenv-src = builtins.getEnv "DEVENV_REPO";
  devenv = (import devenv-src).lib.mkStandardShell;
in
{
  shell = devenv {
    inherit pkgs;
    modules = [ ./devenv.nix ];
  };
}
