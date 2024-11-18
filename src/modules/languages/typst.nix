{ pkgs, config, lib, ... }:

let
  cfg = config.languages.typst;
in
{
  options.languages.typst = {
    enable = lib.mkEnableOption "tools for Typst development";

    package = lib.mkOption {
      type = lib.types.package;
      description = "Which package of Typst to use.";
      default = pkgs.typst;
      defaultText = lib.literalExpression "pkgs.typst";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
      pkgs.tinymist # lsp
      pkgs.typstyle # formatter
    ];
  };
}
