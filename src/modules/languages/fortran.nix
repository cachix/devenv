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
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      fortran-fpm
      fortran-language-server
    ];
  };
}
