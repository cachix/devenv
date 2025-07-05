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

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable Fortran development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable fortran-language-server language server.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.fortran-language-server;
          defaultText = lib.literalExpression "pkgs.fortran-language-server";
          description = "The fortran-language-server package to use.";
        };
      };

      fpm = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable fortran-fpm package manager.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.fortran-fpm;
          defaultText = lib.literalExpression "pkgs.fortran-fpm";
          description = "The fortran-fpm package to use.";
        };
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ] ++ lib.optionals cfg.dev.enable (
      lib.optional (cfg.dev.lsp.enable) cfg.dev.lsp.package ++
        lib.optional (cfg.dev.fpm.enable) cfg.dev.fpm.package
    );
  };
}
