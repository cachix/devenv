{ pkgs
, config
, lib
, ...
}:
let
  cfg = config.languages.assembly;

  asmTargetPkgs = {
    nasm = pkgs.nasm;
    fasm = pkgs.fasm;
    yasm = pkgs.yasm;
  };

  emulatorPkgs = {
    qemu = pkgs.qemu;
    rvvm = pkgs.rvvm;
  };

  isCrossCompile = builtins.elem cfg.type [ "arm" "riscv" ];

  archMapping = {
    nasm = [ "x86_64" ];
    fasm = [ "x86_64" ];
    yasm = [ "x86_64" ];
    arm = [ "aarch64" "armv7l" ];
    riscv = [ "riscv32" "riscv64" ];
  };
  crossArchMap = {
    aarch64 = pkgs.pkgsCross.aarch64-multiplatform;
    armv7l = pkgs.pkgsCross.armv7l-hf-multiplatform;
    riscv32 = pkgs.pkgsCross.riscv32;
    riscv64 = pkgs.pkgsCross.riscv64;
  };

  crossPkgs =
    if isCrossCompile && cfg.targetArch != "x86_64"
    then crossArchMap.${cfg.targetArch} or { }
    else { };

  targetTool = crossPkgs.stdenv.cc or asmTargetPkgs.${cfg.type};
  targetBinutils = crossPkgs.binutils or null;

  isValidArchMapping =
    builtins.hasAttr cfg.type archMapping
    && builtins.elem cfg.targetArch archMapping.${cfg.type};

  rvvmRequiresRiscv =
    !(cfg.emulator.enable
      && cfg.emulator.type == "rvvm"
      && cfg.type != "riscv");
in
{
  options.languages.assembly = {
    enable = lib.mkEnableOption "tools for Assembly Development";

    type = lib.mkOption {
      type = lib.types.enum [ "nasm" "fasm" "yasm" "arm" "riscv" ];
      default = "nasm";
      defaultText = lib.literalExpression "nasm";
      description = ''
        Assembler selection.
        - nasm / fasm / yasm.
        - arm.
        - riscv.
      '';
    };

    targetArch = lib.mkOption {
      type = lib.types.nullOr (lib.types.enum [ "x86_64" "aarch64" "armv7l" "riscv32" "riscv64" ]);
      default =
        if cfg.type == "riscv"
        then "riscv64"
        else if cfg.type == "arm"
        then "aarch64"
        else "x86_64";
      defaultText = lib.literalExpression "x86_64";
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
      default = targetTool;
      defaultText = lib.literalExpression "pkgs.nasm";
      description = ''
        The Assembly packate to use. Override this option to inject
        a custom-built assembler.
      '';
    };

    binutils = lib.mkOption {
      type = lib.types.nullOr lib.types.package;
      default = targetBinutils;
      defaultText = lib.literalExpression "crossPkgs.bintuils or null";
    };

    lsp = {
      enable = lib.mkEnableOption "Assembly Language Server" // { default = true; };
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
          default = isCrossCompile;
          defaultText = lib.literalExpression "isCrossCompile";
        };
      type = lib.mkOption {
        type = lib.types.enum [ "qemu" "rvvm" ];
        default =
          if cfg.type == "riscv"
          then "rvvm"
          else "qemu";
        defaultText = "qemu";
        description = ''
          Select your system emulator backend.
          - qemu: general purpose.
          - rvvm: only RISC-V.
        '';
      };
      package = lib.mkOption {
        type = lib.types.nullOr lib.types.package;
        default = emulatorPkgs.${cfg.emulator.type};
        defaultText = "pkgs.qemu";
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
        assertion = isValidArchMapping;
        message = ''
          Inconsistency between `type` and `targetArch`:
            type       = ${cfg.type}
            targetArch = ${builtins.toString cfg.targetArch}

          Valid mapping:
            nasm/fasm/yasm/custom  → x86_64  | null  (native)
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

    packages =
      [ cfg.package ]
      ++ lib.optional cfg.lsp.enable cfg.lsp.package
      ++ lib.optional (cfg.emulator.enable && cfg.emulator.package != null) cfg.emulator.package;
  };
}
