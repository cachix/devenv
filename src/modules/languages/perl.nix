{ pkgs, config, lib, ... }:

let
  cfg = config.languages.perl;
in
{
  options.languages.perl = {
    enable = lib.mkEnableOption "tools for Perl development";
    packages = lib.mkOption
      {
        type = lib.types.listOf lib.types.str;
        description = "Perl packages to include";
        default = [ ];
        example = [ "Mojolicious" ];
      };

    lsp = {
      enable = lib.mkEnableOption "Perl Language Server" // { default = true; };
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.perlnavigator;
        defaultText = lib.literalExpression "pkgs.perlnavigator";
        description = "The Perl language server package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      (perl.withPackages (p: (with builtins; map
        (pkg: p.${ replaceStrings [ "::" ] [ "" ] pkg })
        cfg.packages)))
    ] ++ lib.optional cfg.lsp.enable cfg.lsp.package;
  };
}
