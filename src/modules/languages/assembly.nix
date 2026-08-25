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

  isValidArchMapping =
    builtins.hasAttr cfg.type archMapping
    && builtins.elem cfg.targetArch archMapping.${cfg.type};

  invalidArchMappingMsg = ''
    languages.assembly: inconsistency between `type` and `targetArch`.
      type       = ${cfg.type}
      targetArch = ${cfg.targetArch}

    Valid mapping:
      nasm/fasm/yasm -> x86_64
      arm            -> aarch64 | armv7l
      riscv          -> riscv64 | riscv32
  '';

  crossPkgs =
    if isCrossCompile && isValidArchMapping
    then crossArchMap.${cfg.targetArch} or { }
    else { };

  defaultPackage =
    if !isValidArchMapping
    then throw invalidArchMappingMsg
    else if isCrossCompile
    then crossPkgs.stdenv.cc
    else asmTargetPkgs.${cfg.type};

  defaultBinutils =
    if isCrossCompile && isValidArchMapping
    then crossPkgs.binutils or null
    else null;

  rvvmRequiresRiscv =
    !(cfg.emulator.enable
      && cfg.emulator.type == "rvvm"
      && cfg.type != "riscv");

  # Reference: https://github.com/bergercookie/asm-lsp#optional-configure-via-asm-lsptoml
  # Valid `assembler` values (upstream): gas | go | masm | nasm | ca65 | avr | fasm | mars
  # Valid `instruction_set` values:      x86 | x86-64 | x86/x86-64 | arm | arm64 | riscv | z80 | 6502 | avr | mips
  asmLspAssemblerMap = {
    nasm = "nasm";
    fasm = "fasm";
    # yasm has no dedicated asm-lsp flavor. Its default parse mode is
    # NASM-compatible syntax, so "nasm" is the closest documented match.
    yasm = "nasm";
    # The arm/riscv cross toolchains here are GCC cross-compilers, whose
    # integrated assembler is GNU `as` (GAS syntax).
    arm = "gas";
    riscv = "gas";
  };

  asmLspInstructionSetMap = {
    x86_64 = "x86-64";
    aarch64 = "arm64";
    armv7l = "arm";
    riscv32 = "riscv";
    riscv64 = "riscv";
  };

  # `default_diagnostics` uses gcc/clang with `-x assembler-with-cpp`
  # and therefore relies on GNU assembler-compatible syntax. This is
  # appropriate for GAS-based toolchains such as ARM/RISC-V, but not
  # for assemblers such as NASM/FASM/YASM. Enabling it for those
  # assemblers would result in false-positive diagnostics.
  asmLspDiagnosticsDefault = cfg.type == "arm" || cfg.type == "riscv";

  asmLspAssembler =
    if cfg.lsp.projectConfig.assembler != null
    then cfg.lsp.projectConfig.assembler
    else asmLspAssemblerMap.${cfg.type};

  asmLspInstructionSet =
    if cfg.lsp.projectConfig.instructionSet != null
    then cfg.lsp.projectConfig.instructionSet
    else
      asmLspInstructionSetMap.${
      cfg.targetArch
      } or (throw ''
        languages.assembly: no known `.asm-lsp.toml` instruction_set
        mapping for targetArch = "${cfg.targetArch}". Set
        `languages.assembly.lsp.projectConfig.instructionSet` explicitly.
      '');

  asmLspDiagnostics =
    if cfg.lsp.projectConfig.diagnostics != null
    then cfg.lsp.projectConfig.diagnostics
    else asmLspDiagnosticsDefault;

  asmLspCompiler =
    if asmLspDiagnostics && isCrossCompile && isValidArchMapping
    then "${cfg.package}/bin/cc"
    else null;

  asmLspBaseSettings = {
    default_config = {
      assembler = asmLspAssembler;
      instruction_set = asmLspInstructionSet;
    };
    opts =
      {
        diagnostics = asmLspDiagnostics;
        default_diagnostics = asmLspDiagnostics;
      }
      # Only wire a `compiler` for cross-toolchains, where `cfg.package`
      # is a cc-wrapper compatible with asm-lsp's `-x assembler-with-cpp` diagnostics.
      // lib.optionalAttrs (asmLspCompiler != null) {
        compiler = asmLspCompiler;
      };
  };

  asmLspSettings = lib.recursiveUpdate asmLspBaseSettings cfg.lsp.projectConfig.extraSettings;
in
{
  options.languages.assembly = {
    enable = lib.mkEnableOption "tools for Assembly Development";

    type = lib.mkOption {
      type = lib.types.enum [ "nasm" "fasm" "yasm" "arm" "riscv" ];
      default = "nasm";
      defaultText = lib.literalExpression "nasm";
      description = ''
        Assembler selection, only one of nasm / fasm / yasm / arm / riscv.

        This setting changes the default value of `targetArch` if you use `arm` or `riscv`.
      '';
    };

    targetArch = lib.mkOption {
      type = lib.types.enum [ "x86_64" "aarch64" "armv7l" "riscv32" "riscv64" ];
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
      default = defaultPackage;
      defaultText = lib.literalExpression "pkgs.nasm";
      description = ''
        Assembly toolchain/package to use.

        For ARM/RISC-V this defaults to the cross compiler
        selected from targetArch, but can be overridden explicitly.
      '';
    };

    binutils = lib.mkOption {
      type = lib.types.nullOr lib.types.package;
      default = defaultBinutils;
      defaultText = lib.literalExpression "crossPkgs.bintuils or null";
      description = ''
        Cross binutils package used alongside `package` when `type` is
        "arm" or "riscv", `null` on native (nasm/fasm/yasm) configurations.
      '';
    };

    lsp = {
      enable = lib.mkEnableOption "Assembly Language Server" // { default = cfg.enable; };
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.asm-lsp;
        defaultText = lib.literalExpression "pkgs.asm-lsp";
        description = "The Assembly Language Server package to use.";
      };

      projectConfig = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = cfg.lsp.enable;
          defaultText = lib.literalExpression "config.languages.assembly.lsp.enable";
          description = ''
            Whether to materialize a `.asm-lsp.toml` file at the workspace
            root (`$DEVENV_ROOT`) on `devenv shell`/`enterShell`, derived
            automatically from `languages.assembly.type` and `targetArch`.

            The file is only rewritten when its content actually changes,
            so it is safe to commit and won't dirty `git status` on every
            shell entry.

            Reference: https://github.com/bergercookie/asm-lsp#optional-configure-via-asm-lsptoml
          '';
        };

        assembler = lib.mkOption {
          type = lib.types.nullOr (lib.types.enum [ "gas" "go" "masm" "nasm" "ca65" "avr" "fasm" "mars" ]);
          default = null;
          defaultText = lib.literalExpression "derived from `languages.assembly.type`";
          description = ''
            Override the asm-lsp `assembler` flavor. Leave `null` to derive
            it automatically:
              nasm → "nasm"
              fasm → "fasm"
              yasm → "nasm"  (closest match; yasm has no dedicated flavor)
              arm / riscv → "gas" (the cross toolchain drives GNU as)
          '';
        };

        instructionSet = lib.mkOption {
          type = lib.types.nullOr (lib.types.enum [ "x86" "x86-64" "x86/x86-64" "arm" "arm64" "riscv" "z80" "6502" "avr" "mips" ]);
          default = null;
          defaultText = lib.literalExpression "derived from `languages.assembly.targetArch`";
          description = ''
            Override the asm-lsp `instruction_set`. Leave `null` to derive
            it automatically from `targetArch`:
              x86_64  → "x86-64"
              aarch64 → "arm64"
              armv7l  → "arm"
              riscv32 / riscv64 → "riscv"
          '';
        };

        diagnostics = lib.mkOption {
          type = lib.types.nullOr lib.types.bool;
          default = null;
          defaultText = lib.literalExpression ''type == "arm" || type == "riscv"'';
          description = ''
            Whether asm-lsp should shell out to a C compiler for inline
            diagnostics (`opts.diagnostics` / `opts.default_diagnostics`).
            Left `null`, this is enabled only for `arm`/`riscv`: their
            cross toolchains emit GAS syntax, which is what asm-lsp's
            diagnostic compiler expects. For `nasm`/`fasm`/`yasm` (Intel
            syntax) it defaults to disabled, since gcc/clang's integrated
            assembler cannot parse that syntax and would otherwise report
            spurious errors.
          '';
        };

        extraSettings = lib.mkOption {
          type = lib.types.attrsOf lib.types.anything;
          default = { };
          description = ''
            Extra settings recursively merged into the generated
            `.asm-lsp.toml` (e.g. to declare `[[project]]` overrides for
            sub-directories, or a `[opts] compile_flags_txt`).
          '';
          example = lib.literalExpression ''
            {
              project = [
                {
                  path = "boot";
                  assembler = "nasm";
                  instruction_set = "x86";
                }
              ];
            }
          '';
        };
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
        defaultText = lib.literalExpression "qemu";
        description = ''
          Select your system emulator backend.
          - qemu: general purpose.
          - rvvm: only RISC-V.
        '';
      };
      package = lib.mkOption {
        type = lib.types.package;
        default = emulatorPkgs.${cfg.emulator.type};
        defaultText = lib.literalExpression "pkgs.qemu";
        description = ''
          System emulator, solved by `emulator.type`, but you can overwrite
          for an emulator of your choice.
        '';
      };
    };
  };
  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = isValidArchMapping;
        message = invalidArchMappingMsg;
      }
      {
        assertion = rvvmRequiresRiscv;
        message = ''
          RVVM is exclusive to RISC-V. Change `languages.assembly.type` to "riscv"
          or use `emulator.type = "qemu"`.
        '';
      }
      (
        let
          packageEval = builtins.tryEval cfg.package;
        in
        {
          assertion = !packageEval.success || lib.isDerivation packageEval.value;
          message = ''
            `languages.assembly.package` must be a derivation (got: ${builtins.typeOf cfg.package}).
            Pass a package such as `pkgs.asm`, or a cross `stdenv.cc`, not a raw string or plain
            attribute set.
          '';
        }
      )
      {
        assertion = cfg.binutils == null || lib.isDerivation cfg.binutils;
        message = ''
          `languages.assembly.binutils`, when set, must be a derivation
          (got: ${builtins.typeOf cfg.binutils}).
        '';
      }
      {
        assertion = cfg.emulator.package == null || lib.isDerivation cfg.emulator.package;
        message = ''
          `languages.assembly.emulator.package`, when set, must be a
          derivation (got: ${builtins.typeOf cfg.emulator.package}).
        '';
      }
    ];

    files = lib.mkIf (cfg.lsp.enable && cfg.lsp.projectConfig.enable) {
      ".asm-lsp.toml".toml = asmLspSettings;
    };

    packages =
      [ cfg.package ]
      ++ lib.optional cfg.lsp.enable cfg.lsp.package
      ++ lib.optional cfg.emulator.enable cfg.emulator.package;
  };
}
