{ pkgs, config, lib, ... }:

let
  cfg = config.languages.r;
in
{
  options.languages.r = {
    enable = lib.mkEnableOption "tools for R development";
    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.R;
      defaultText = lib.literalExpression "pkgs.R";
      description = "The R package to use.";
    };
    radian = {
      enable = lib.mkEnableOption "a 21 century R console";
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.radianWrapper;
        defaultText = lib.literalExpression "pkgs.radianWrapper";
        description = "The radian package to use.";
      };
    };
    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = ''
          Enable R development tools.
          
          Note: R development tools like languageserver (LSP), styler (formatter),
          and lintr (linter) are typically installed via R's package manager.
          
          To install these tools, run R and execute:
          install.packages(c("languageserver", "styler", "lintr"))
          
          For more tools, consider:
          - dev: for package development
          - testthat: for unit testing
          - roxygen2: for documentation generation
        '';
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [ cfg.package ] ++ lib.lists.optional cfg.radian.enable cfg.radian.package;
  };
}
