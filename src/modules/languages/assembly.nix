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
      type = lib.types.enum ["nasm" "fasm" "yasm" "arm" "riscv" "custom"];
      default = "nasm";
      description = ''
        Assembler selection.
        - nasm / fasm / yasm.
        - arm.
        - riscv.
        - custom: User-provided assembly tool.
      '';
    };

    targetArch = lib.mkOption {
      type = lib.types.nullOr (lib.types.enum ["aarch64" "armv7l" "riscv32" "riscv64"]);
      default = "aarch64";
      description = ''
        Target architecture.

        Applies only when type is "arm" or "riscv".

        ARM family (type = "arm"):
        - aarch64: ARM 64-bit (RPi 4, Pi 5, Apple Silicon and more).
        - armv7l: ARM 32-bit (RPi 2, 3 in 32-bit, embedded systems).
        RISC-V family (type = "riscv"):
        - riscv32
        - riscv64
      '';
    };

    package = lib.mkOption {
      type = lib.types.package;
      default = let
        armTargetPkgs =
          if
            cfg.type
            == "arm"
            && cfg.targetArch
            == "aarch64"
          then pkgs.pkgsCross.aarch64-multiplatform.stdenv.cc
          else pkgs.pkgsCross.armv7l-hf-multiplatform.stdenv.cc;
        riscvTargetPkgs =
          if cfg.type == "riscv" && cfg.targetArch == "riscv64"
          then pkgs.pkgsCross.riscv64.stdenv.cc
          else pkgs.pkgsCross.riscv32.stdenv.cc;
        packages = {
          "nasm" = pkgs.nasm;
          "fasm" = pkgs.fasm;
          "yasm" = pkgs.yasm;
        };
      in
        if cfg.type == "arm"
        then armTargetPkgs
        else if cfg.type == "riscv"
        then riscvTargetPkgs
        else if cfg.type == "custom"
        then pkgs.nasm
        else packages.${config.languages.assembly.type} or pkgs.nasm;
      defaultText = lib.literalExpression "pkgs.nasm";
      description = ''
        The Assembly packate to use. Override this option to inject
        a custom-built assembler.

        e.g: languages.assembly.package = pkgs.callPackage ./my-custom-nasm {};

        When type="custom", this defaults to nasm but EXPECTS user override.
      '';
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
