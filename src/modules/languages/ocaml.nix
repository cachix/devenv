{ pkgs, config, lib, ... }:

let
  cfg = config.languages.ocaml;
in
{
  options.languages.ocaml = {
    enable = lib.mkEnableOption "tools for OCaml development";

    packages = lib.mkOption
      {
        type = lib.types.attrs;
        description = "The package set of OCaml to use";
        default = pkgs.ocaml-ng.ocamlPackages;
        defaultText = lib.literalExpression "pkgs.ocaml-ng.ocamlPackages_4_12";
      };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.packages.ocaml
      cfg.packages.dune_3
      cfg.packages.ocaml-lsp
      cfg.packages.merlin
      cfg.packages.utop
      cfg.packages.odoc
      cfg.packages.ocp-indent
      cfg.packages.findlib
      pkgs.ocamlformat
    ];
  };
}
