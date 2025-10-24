let
  sources = import ./npins;
  pkgs = import sources.nixpkgs { };
  devenv = (import ../..).lib.nonFlakeMkShell ./.;
in
{
  shell = devenv {
    inherit pkgs;
    modules = [ ./devenv.nix ];
  };
}
