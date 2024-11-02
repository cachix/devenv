{ pkgs, name, version, ... }:
pkgs.buildGoApplication {
  pname = name;
  version = version;

  src = builtins.path {
    path = ./.;
    name = "source";
  };

  ## remember to call 'gomod2nix' to generate this file
  modules = ./gomod2nix.toml;
}
