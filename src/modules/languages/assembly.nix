{ pkgs, config, lib, ... }:

let
  cfg = config.languages.assembly;
in
{
  options.languages.assembly = {
    enable = lib.mkEnableOption "tools for Assembly development";

    assembler = lib.mkOption {
      type = lib.types.enum [ "nasm" "yasm" "fasm" "gas" ];
      default = "nasm";
      description = "Primary assembler to use.";
    };

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.${cfg.assembler};
      defaultText = lib.literalExpression "pkgs.\${cfg.assembler}";
      description = "The primary assembler package to use.";
    };

    additionalAssemblers = lib.mkOption {
      type = lib.types.listOf (lib.types.enum [ "nasm" "yasm" "fasm" "gas" ]);
      default = [ ];
      description = "Additional assemblers to include in the environment.";
      example = [ "yasm" "fasm" ];
    };

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable Assembly development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable asm-lsp language server.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.asm-lsp;
          defaultText = lib.literalExpression "pkgs.asm-lsp";
          description = "The asm-lsp package to use.";
        };
      };

      formatter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable assembly formatters.";
        };
        nasmfmt = {
          enable = lib.mkOption {
            type = lib.types.bool;
            default = true;
            description = "Enable nasmfmt formatter.";
          };
          package = lib.mkOption {
            type = lib.types.package;
            default = pkgs.nasmfmt;
            defaultText = lib.literalExpression "pkgs.nasmfmt";
            description = "The nasmfmt package to use.";
          };
        };
        asmfmt = {
          enable = lib.mkOption {
            type = lib.types.bool;
            default = false;
            description = "Enable asmfmt formatter.";
          };
          package = lib.mkOption {
            type = lib.types.package;
            default = pkgs.asmfmt;
            defaultText = lib.literalExpression "pkgs.asmfmt";
            description = "The asmfmt package to use.";
          };
        };
      };

      disassembler = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable disassembly tools.";
        };
        radare2 = {
          enable = lib.mkOption {
            type = lib.types.bool;
            default = true;
            description = "Enable radare2 disassembler.";
          };
          package = lib.mkOption {
            type = lib.types.package;
            default = pkgs.radare2;
            defaultText = lib.literalExpression "pkgs.radare2";
            description = "The radare2 package to use.";
          };
        };
        capstone = {
          enable = lib.mkOption {
            type = lib.types.bool;
            default = true;
            description = "Enable capstone disassembler.";
          };
          package = lib.mkOption {
            type = lib.types.package;
            default = pkgs.capstone;
            defaultText = lib.literalExpression "pkgs.capstone";
            description = "The capstone package to use.";
          };
        };
      };

      debugger = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable gdb debugger.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.gdb;
          defaultText = lib.literalExpression "pkgs.gdb";
          description = "The debugger package to use.";
        };
      };

      binutils = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable binutils tools.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.binutils;
          defaultText = lib.literalExpression "pkgs.binutils";
          description = "The binutils package to use.";
        };
      };

      hexEditor = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = false;
          description = "Enable ghex hex editor.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.ghex;
          defaultText = lib.literalExpression "pkgs.ghex";
          description = "The hex editor package to use.";
        };
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ] ++ (map (asm: pkgs.${asm}) cfg.additionalAssemblers)
    ++ lib.optionals cfg.dev.enable (
      lib.optional cfg.dev.lsp.enable cfg.dev.lsp.package ++
        lib.optionals cfg.dev.formatter.enable (
          lib.optional cfg.dev.formatter.nasmfmt.enable cfg.dev.formatter.nasmfmt.package ++
            lib.optional cfg.dev.formatter.asmfmt.enable cfg.dev.formatter.asmfmt.package
        ) ++
        lib.optionals cfg.dev.disassembler.enable (
          lib.optional cfg.dev.disassembler.radare2.enable cfg.dev.disassembler.radare2.package ++
            lib.optional cfg.dev.disassembler.capstone.enable cfg.dev.disassembler.capstone.package
        ) ++
        lib.optional cfg.dev.debugger.enable cfg.dev.debugger.package ++
        lib.optional cfg.dev.binutils.enable cfg.dev.binutils.package ++
        lib.optional cfg.dev.hexEditor.enable cfg.dev.hexEditor.package
    );

    enterShell = ''
      echo "Assembly development environment:"
      echo "  Primary assembler: ${cfg.assembler}"
      ${lib.optionalString (cfg.additionalAssemblers != []) ''
        echo "  Additional assemblers: ${lib.concatStringsSep ", " cfg.additionalAssemblers}"
      ''}
      ${lib.optionalString cfg.dev.enable ''
        echo "  Development tools enabled:"
        ${lib.optionalString cfg.dev.lsp.enable ''echo "    - LSP: asm-lsp"''}
        ${lib.optionalString cfg.dev.formatter.enable ''
          ${lib.optionalString cfg.dev.formatter.nasmfmt.enable ''echo "    - NASM formatter: nasmfmt"''}
          ${lib.optionalString cfg.dev.formatter.asmfmt.enable ''echo "    - Go ASM formatter: asmfmt"''}
        ''}
        ${lib.optionalString cfg.dev.disassembler.enable ''
          ${lib.optionalString cfg.dev.disassembler.radare2.enable ''echo "    - Reverse engineering: radare2"''}
          ${lib.optionalString cfg.dev.disassembler.capstone.enable ''echo "    - Disassembler library: capstone"''}
        ''}
        ${lib.optionalString cfg.dev.debugger.enable ''echo "    - Debugger: gdb"''}
        ${lib.optionalString cfg.dev.binutils.enable ''echo "    - Binary utilities: binutils"''}
        ${lib.optionalString cfg.dev.hexEditor.enable ''echo "    - Hex editor: ghex"''}
      ''}
    '';
  };
}
