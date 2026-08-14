{
  pkgs,
  config,
  lib,
  ...
}: let
  cfg = config.languages.ada;

  compilerPackages = {
    gnat13 = pkgs.gnat13Packages.gnat;
    gnat14 = pkgs.gnat14Packages.gnat;
    gnat15 = pkgs.gnat15Packages.gnat;
    gnat16 = pkgs.gnat16Packages.gnat;
  };

  gnatCompiler = compilerPackages.${cfg.version};
in {
  options.languages.ada = {
    enable = lib.mkEnableOption "tools for Ada Development";

    version = lib.mkOption {
      type = lib.types.enum ["gnat13" "gnat14" "gnat15" "gnat16"];
      default = "gnat13";
      defaultText = lib.literalExpression "gnat13";
      description = ''
        GNAT compiler version to use for Ada development.
      '';
    };

    package = lib.mkOption {
      type = lib.types.package;
      default = gnatCompiler;
      defaultText = lib.literalExpression "pkgs.gnat13Packages.gnat";
      description = ''
        GNAT Compiler package used to build Ada projects.
      '';
    };

    gprbuild = {
      enable =
        lib.mkEnableOption "Ada Multi-language extensible build tool."
        // {
          default = cfg.enable == true;
        };
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.gnatPackages.gprbuild;
        defaultText = lib.literalExpression "pkgs.gnatPackages.gprbuild";
        description = ''
          GPRbuild package used to build Ada and multi-language projects.
        '';
      };
    };
    gpr2 = {
      enable = lib.mkEnableOption "Ada Framework for analyzing the GNAT Project (GPR) files";
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.gnatPackages.gpr2;
        defaultText = lib.literalExpression "pkgs.gnatPackages.gpr2";
        description = ''
          GPR2 package for working with and analyzing GNAT Project files.
        '';
      };
    };
    xmlada = {
      enable = lib.mkEnableOption "XML/Ada: An XML Parser for Ada";
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.gnatPackages.xmlada;
        defaultText = lib.literalExpression "pkgs.gnatPackages.xmlada";
        description = ''
          XML/Ada package providing XML parsing support for Ada projects.
        '';
      };
    };
  };
  config = lib.mkIf cfg.enable {
    packages = with pkgs;
      [cfg.package]
      ++ lib.optional cfg.gpr2.enable cfg.gpr2.package
      ++ lib.optional cfg.xmlada.enable cfg.xmlada.package;
  };
}
