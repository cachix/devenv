let
  sources = import ./npins;
  pkgs = import sources.nixpkgs { };
  devenv = (import ../..).lib.nonFlakeMkShell ./.;
  flake-compat = import sources.flake-compat;
in
{
  shell = devenv {
    inherit pkgs;
    modules = [ ./devenv.nix ];
    inputs = {
      nixpkgs = pkgs;
      rust-overlay = (flake-compat { src = sources.rust-overlay; }).defaultNix;
    };
  };
}
