{ pkgs, lib, ... }:

{
  treefmt = {
    enable = true;

    config.programs = {
      nixpkgs-fmt.enable = true;
      nixfmt.enable = true;
    };
  };

  enterTest = ''
    treefmt
  '';
}
