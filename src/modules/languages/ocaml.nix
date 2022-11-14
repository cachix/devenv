{ pkgs, config, lib, ... }:

let
  cfg = config.languages.ocaml;
in
{
  options.languages.ocaml = {
    enable = lib.mkEnableOption "Enable tools for OCaml development.";
  };

  config = lib.mkIf cfg.enable {
    packages = [
      pkgs.ocaml
      pkgs.ocaml-ng.ocamlPackages.dune_3
      pkgs.ocaml-ng.ocamlPackages.ocaml-lsp
    ];

    enterShell = ''
    '';
  };
}
