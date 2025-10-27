let
  sources = import ./npins;
  pkgs = import sources.nixpkgs { };
  devenv = (import sources.devenv).lib.nonFlakeMkShell ./.;
in
{
  shell = devenv {
    inherit pkgs;
    modules = [ ./devenv.nix ];
  };
}
