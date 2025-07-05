{ pkgs, config, lib, ... }:

let
  cfg = config.languages.cobol;
in
{
  options.languages.cobol = {
    enable = lib.mkEnableOption "tools for COBOL development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.gnucobol;
      defaultText = lib.literalExpression "pkgs.gnucobol";
      description = "The GNU COBOL compiler package to use.";
    };

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable COBOL development tools.";
      };

      editor = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable cobol editor support.";
        };
        emacsMode = {
          enable = lib.mkOption {
            type = lib.types.bool;
            default = true;
            description = "Enable cobol-mode for Emacs.";
          };
          package = lib.mkOption {
            type = lib.types.nullOr lib.types.package;
            default = if (builtins.hasAttr "cobol-mode" pkgs.emacsPackages) then pkgs.emacsPackages.cobol-mode else null;
            defaultText = lib.literalExpression "pkgs.emacsPackages.cobol-mode or null";
            description = "The cobol-mode package to use.";
          };
        };
      };

      documentation = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = false;
          description = "Enable cobol documentation tools.";
        };
        robodoc = {
          enable = lib.mkOption {
            type = lib.types.bool;
            default = true;
            description = "Enable robodoc documentation tool.";
          };
          package = lib.mkOption {
            type = lib.types.package;
            default = pkgs.robodoc;
            defaultText = lib.literalExpression "pkgs.robodoc";
            description = "The ROBODoc package to use.";
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

      linter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = false;
          description = ''
            Enable cobol linter.
          '';
        };
      };
    };

    copybooks = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      description = ''
        List of directories to search for COBOL copybooks.
        These will be added to the COB_COPY_DIR environment variable.
      '';
      example = [ "./copybooks" "../shared-copybooks" ];
    };

    compilerFlags = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      description = "Additional flags to pass to the GNU COBOL compiler.";
      example = [ "-Wall" "-std=cobol2014" "-free" ];
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ] ++ lib.optionals cfg.dev.enable (
      lib.optionals cfg.dev.editor.enable
        (
          lib.optional (cfg.dev.editor.emacsMode.enable && cfg.dev.editor.emacsMode.package != null) cfg.dev.editor.emacsMode.package
        ) ++
      lib.optionals cfg.dev.documentation.enable (
        lib.optional cfg.dev.documentation.robodoc.enable cfg.dev.documentation.robodoc.package
      ) ++
      lib.optional (cfg.dev.debugger.enable && lib.meta.availableOn pkgs.stdenv.hostPlatform pkgs.gdb) cfg.dev.debugger.package
    );

    # Enable C toolchain since GNU COBOL compiles to C
    languages.c.enable = lib.mkDefault true;

    env = {
      # Set copybook search directories
      COB_COPY_DIR = lib.optionalString (cfg.copybooks != [ ])
        (lib.concatStringsSep ":" cfg.copybooks);

      # Set compiler flags if specified
      COB_CFLAGS = lib.optionalString (cfg.compilerFlags != [ ])
        (lib.concatStringsSep " " cfg.compilerFlags);

      # Set library path for GNU COBOL runtime
      COB_LIBRARY_PATH = "${cfg.package}/lib";

      # Configuration directory for GNU COBOL
      COB_CONFIG_DIR = "${cfg.package}/share/gnucobol/config";
    };

    enterShell = ''
      echo "COBOL development environment activated!"
      echo "GNU COBOL compiler: $(command -v cobc 2>/dev/null && cobc --version | head -1 || echo 'cobc not found')"
      ${lib.optionalString (cfg.copybooks != [ ]) ''
        echo "Copybook directories: ${lib.concatStringsSep ", " cfg.copybooks}"
      ''}
      ${lib.optionalString (cfg.compilerFlags != [ ]) ''
        echo "Compiler flags: ${lib.concatStringsSep " " cfg.compilerFlags}"
      ''}
      ${lib.optionalString cfg.dev.enable ''
        echo "Development tools:"
        ${lib.optionalString (cfg.dev.editor.emacsMode.enable && cfg.dev.editor.emacsMode.package != null) ''
          echo "  - Emacs COBOL mode available"
        ''}
        ${lib.optionalString (cfg.dev.documentation.enable && cfg.dev.documentation.robodoc.enable) ''
          echo "  - Documentation: $(command -v robodoc 2>/dev/null && echo 'robodoc available' || echo 'robodoc not found')"
        ''}
        ${lib.optionalString cfg.dev.debugger.enable ''
          echo "  - Debugger: $(command -v gdb 2>/dev/null && gdb --version | head -1 || echo 'gdb not found')"
        ''}
      ''}
      echo ""
      echo "Quick start:"
      echo "  Create a COBOL program: example.cob"
      echo "  Compile: cobc -x -o example example.cob"
      echo "  Run: ./example"
      ${lib.optionalString (cfg.dev.documentation.enable && cfg.dev.documentation.robodoc.enable) ''
        echo "  Generate docs: robodoc --src ./ --doc ./docs --multidoc --html"
      ''}
    '';
  };
}
