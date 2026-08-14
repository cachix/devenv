{
  pkgs,
  config,
  lib,
  ...
}: let
  cfg = config.languages.assembly;

  crossPkgs =
    if cfg.type == "arm"
    then
      if cfg.targetArch == "aarch64"
      then pkgs.pkgsCross.aarch64-multiplatform
      else if cfg.targetArch == "armv7l"
      then pkgs.pkgsCross.armv7l-hf-multiplatform
      else null
    else if cfg.type == "riscv"
    then
      if cfg.targetArch == "riscv64"
      then pkgs.pkgsCross.riscv64
      else if cfg.targetArch == "riscv32"
      then pkgs.pkgsCross.riscv32
      else null
    else null;

  targetCompiler =
    if crossPkgs != null
    then crossPkgs.stdenv.cc
    else null;
  targetBinutils =
    if crossPkgs != null
    then crossPkgs.binutils
    else null;

  asmTargetPkgs = {
    "nasm" = pkgs.nasm;
    "fasm" = pkgs.fasm;
    "yasm" = pkgs.yasm;
  };

  inconsistentAsmAndTargetArch =
    if cfg.type == "arm"
    then builtins.elem cfg.targetArch ["aarch64" "armv7l"]
    else if cfg.type == "riscv"
    then builtins.elem cfg.targetArch ["riscv32" "riscv64"]
    else if builtins.elem cfg.type ["nasm" "fasm" "yasm" "custom"]
    then cfg.targetArch == "x86_64" || cfg.targetArch == null
    else true;

  rvvmRequiresRiscv =
    !(cfg.emulator.enable
      && cfg.emulator.type == "rvvm"
      && cfg.type != "riscv");
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
      type = lib.types.nullOr (lib.types.enum ["x86_64" "aarch64" "armv7l" "riscv32" "riscv64"]);
      default = "x86_64";
      description = ''
        Target architecture.

        Default (developer host assumed): x86_64

        Applies only when type is "arm" or "riscv".

        ARM family (type = "arm"):
        - aarch64: ARM 64-bit (RPi 4, Pi 5, Apple Silicon and more).
        - armv7l: ARM 32-bit (RPi 2, 3 in 32-bit, embedded systems).
        RISC-V family (type = "riscv"):
        - riscv32 / riscv64
      '';
    };

    package = lib.mkOption {
      type = lib.types.package;
      default =
        if targetCompiler != null
        then targetCompiler
        else if cfg.type == "custom"
        then pkgs.nasm
        else asmTargetPkgs.${cfg.type} or pkgs.nasm;
      defaultText = lib.literalExpression "pkgs.nasm";
      description = ''
        The Assembly packate to use. Override this option to inject
        a custom-built assembler.

        e.g: languages.assembly.package = pkgs.callPackage ./my-custom-nasm {};

        When type="custom", this defaults to nasm but EXPECTS user override.
      '';
    };

    binutils = lib.mkOption {
      type = lib.types.nullOr lib.types.package;
      default =
        if targetBinutils != null
        then targetBinutils
        else null;
      defaultText = lib.literalExpression "crossPkgs.binutils";
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

    emulator = {
      enable =
        lib.mkEnableOption "System Emulator (qemu / rvvm)."
        // {
          default = cfg.type == "arm" || cfg.type == "riscv";
          defaultText = lib.literalExpression ''
            languages.assembly.type == "arm" || languages.assembly.type == "riscv"
          '';
        };
      type = lib.mkOption {
        type = lib.types.nullOr (lib.types.enum ["qemu" "rvvm"]);
        default = "qemu";
        description = ''
          Select your system emulator backend.
          - qemu: general purpose.
          - rvvm: only RISC-V.
          - null: disable automatic 'package' selection, useful if you
                  are going to override 'package' manually.
        '';
      };
      package = lib.mkOption {
        type = lib.types.nullOr lib.types.package;
        default = let
          emulatorPkgs = {
            "qemu" = pkgs.qemu;
            "rvvm" = pkgs.rvvm;
          };
        in
          if cfg.emulator.type == null
          then null
          else emulatorPkgs.${cfg.emulator.type} or pkgs.qemu;
        description = ''
          System emulator, solved by `emulator.type`, but you can overwrite
          for an emulator of your choice.

          e.g:
           - languages.assembly.emulator.package = pkgs.callPackage  ./my-pkg.nix {};
           - languages.assembly.emulator.package = pkgs.<pkg-name>;
        '';
      };
    };
  };
  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = inconsistentAsmAndTargetArch;
        message = ''
          Inconsistency between `type` and `targetArch`:
            type       = ${cfg.type}
            targetArch = ${builtins.toString cfg.targetArch}

          Valid mapping:
            nasm/fasm/yasm/custom → x86_64 | null  (native)
            arm                    → aarch64 | armv7l
            riscv                  → riscv32 | riscv64
        '';
      }
      {
        assertion = rvvmRequiresRiscv;
        message = ''
          RVVM is exclusive to RISC-V. Change `languages.assembly.type` to "riscv"
          or use `emulator.type = "qemu"`.
        '';
      }
    ];

    packages = with pkgs;
      [cfg.package]
      ++ lib.optional cfg.lsp.enable cfg.lsp.package
      ++ lib.optional (cfg.emulator.enable && cfg.emulator.package != null) cfg.emulator.package;
  };
}
