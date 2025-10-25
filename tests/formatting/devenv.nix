{ pkgs, lib, ... }:

{
  formatting = {
    enable = true;

    treefmt.programs = {
      nixpkgs-fmt.enable = true;
      nixfmt.enable = true;
    };
  };

  enterTest = ''
    treefmt
  '';
}
