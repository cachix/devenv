{ pkgs, config, lib, ... }:

let
  cfg = config.languages.fortran;
in
{
  options.languages.fortran = {
    enable = lib.mkEnableOption "tools for Fortran Development.";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.gfortran;
      defaultText = lib.literalExpression "pkgs.gfortran";
      description = "The Fortran package to use.";
    };

    lsp = {
      enable = lib.mkEnableOption "Fortran Language Server" // { default = true; };
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.fortls;
        defaultText = lib.literalExpression "pkgs.fortls";
        description = "The Fortran language server package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      cfg.package
      fortran-fpm
    ] ++ lib.optional cfg.lsp.enable cfg.lsp.package;
  };
}
