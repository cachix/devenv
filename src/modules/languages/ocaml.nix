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

    lsp = {
      enable = lib.mkEnableOption "OCaml Language Server" // { default = true; };
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.ocamlPackages.ocaml-lsp;
        defaultText = lib.literalExpression "pkgs.ocamlPackages.ocaml-lsp";
        description = "The OCaml language server package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.packages.ocaml
      cfg.packages.dune_3
      cfg.packages.merlin
      cfg.packages.utop
      cfg.packages.odoc
      cfg.packages.ocp-indent
      cfg.packages.findlib
      pkgs.ocamlformat
    ] ++ lib.optional cfg.lsp.enable cfg.lsp.package;
  };
}
