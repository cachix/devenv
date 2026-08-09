{
  pkgs,
  config,
  lib,
  ...
}: let
  cfg = config.languages.assembly;
in {
  options.languages.assembly = {
    enable = lib.mkEnableOption "tools for Assembly Development";

    type = lib.mkOption {
      type = lib.types.enum ["nasm" "fasm" "yasm"];
      default = "nasm";
      description = ''
        The Assembler Compiler to use.
        - nasm.
        - fasm.
        - yasm.
      '';
    };

    package = lib.mkOption {
      type = lib.types.package;
      default = let
        packages = {
          "nasm" = pkgs.nasm;
          "fasm" = pkgs.fasm;
          "yasm" = pkgs.yasm;
        };
      in
        packages.${config.languages.assembly.type} or pkgs.nasm;
      defaultText = lib.literalExpression "pkgs.nasm";
      description = "The Assembly packate to use.";
    };

    lsp = {
      enable = lib.mkEnableOption "Assembly Language Server" // {default = true;};
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.asm-lsp;
        defaultText = lib.literalExpression "pkgs.asm-lsp";
        description = "The Assembly Language Server package to use.";
      };
    };
  };
  config = lib.mkIf cfg.enable {
    packages = with pkgs; [cfg.package] ++ lib.optional cfg.lsp.enable cfg.lsp.package;
  };
}
