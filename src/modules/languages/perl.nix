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

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable Perl development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable Perl language server (PerlLanguageServer).";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.perlPackages.PerlLanguageServer;
          defaultText = lib.literalExpression "pkgs.perlPackages.PerlLanguageServer";
          description = "The PerlLanguageServer package to use.";
        };
      };

      formatter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable Perl formatter (perltidy).";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.perlPackages.PerlTidy;
          defaultText = lib.literalExpression "pkgs.perlPackages.PerlTidy";
          description = "The perltidy package to use.";
        };
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      (perl.withPackages (p: (with builtins; map
        (pkg: p.${ replaceStrings [ "::" ] [ "" ] pkg })
        cfg.packages)))
    ] ++ lib.optionals cfg.dev.enable (
      lib.optional cfg.dev.lsp.enable cfg.dev.lsp.package ++
        lib.optional cfg.dev.formatter.enable cfg.dev.formatter.package
    );
  };
}
