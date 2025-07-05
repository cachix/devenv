{ pkgs, config, lib, ... }:

let
  cfg = config.languages.ada;

  # Available GNAT versions in nixpkgs
  gnatVersions = {
    "12" = pkgs.gnat12;
    "13" = pkgs.gnat13;
  };

  # Get the GNAT package based on version
  gnatPackage = gnatVersions.${cfg.version} or (throw "GNAT version ${cfg.version} is not available. Available versions: ${lib.concatStringsSep ", " (builtins.attrNames gnatVersions)}");

  # Build function that ensures Ada packages are built with the selected GNAT version
  buildWithGnat = gnatPkg: adaPkg: adaPkg.override { gnat = gnatPkg; };

  # Get GNAT packages ecosystem for the selected version
  gnatPackages = pkgs."gnat${cfg.version}Packages" or (throw "GNAT ${cfg.version} packages not available");
in
{
  options.languages.ada = {
    enable = lib.mkEnableOption "tools for Ada development";

    version = lib.mkOption {
      type = lib.types.enum [ "12" "13" ];
      default = "13";
      description = ''
        The GNAT compiler version to use.
        
        GNAT 13 is the latest and recommended version for new projects.
        GNAT 12 provides a stable alternative for legacy compatibility.
      '';
    };

    package = lib.mkOption {
      type = lib.types.package;
      default = gnatPackage;
      defaultText = lib.literalExpression "gnat\${languages.ada.version}";
      description = "The GNAT compiler package to use.";
    };

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable Ada development tools.";
      };

      gprbuild = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable gprbuild build tool.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = gnatPackages.gprbuild;
          defaultText = lib.literalExpression "gnat\${languages.ada.version}Packages.gprbuild";
          description = "The GPRbuild package to use.";
        };
      };

      gnatcoll = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable gnatcoll-core libraries.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = gnatPackages.gnatcoll-core;
          defaultText = lib.literalExpression "gnat\${languages.ada.version}Packages.gnatcoll-core";
          description = "The GNATCOLL core package to use.";
        };
      };

      gnatcoll-bindings = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = false;
          description = "Enable additional GNATCOLL bindings (database, compression, etc.).";
        };
        packages = lib.mkOption {
          type = lib.types.listOf lib.types.package;
          default = with gnatPackages; [
            gnatcoll-sql
            gnatcoll-sqlite
            gnatcoll-postgres
            gnatcoll-gmp
            gnatcoll-zlib
            gnatcoll-lzma
            gnatcoll-readline
            gnatcoll-iconv
            gnatcoll-python3
            gnatcoll-syslog
            gnatcoll-omp
          ];
          defaultText = lib.literalExpression ''
            with gnat''${languages.ada.version}Packages; [
              gnatcoll-sql gnatcoll-sqlite gnatcoll-postgres
              gnatcoll-gmp gnatcoll-zlib gnatcoll-lzma
              gnatcoll-readline gnatcoll-iconv gnatcoll-python3
              gnatcoll-syslog gnatcoll-omp
            ]
          '';
          description = "Additional GNATCOLL binding packages to include.";
        };
      };

      spark = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = false;
          description = "Enable SPARK formal verification tools (if available).";
        };
        package = lib.mkOption {
          type = lib.types.nullOr lib.types.package;
          default = if (builtins.hasAttr "gnatprove" gnatPackages) then gnatPackages.gnatprove else null;
          defaultText = lib.literalExpression "gnat\${languages.ada.version}Packages.gnatprove or null";
          description = "The SPARK/gnatprove package to use (if available).";
        };
      };

      gpr2 = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = false;
          description = "Enable GPR2 library (next-generation GPR library).";
        };
        package = lib.mkOption {
          type = lib.types.nullOr lib.types.package;
          default = if (builtins.hasAttr "gpr2" gnatPackages) then gnatPackages.gpr2 else null;
          defaultText = lib.literalExpression "gnat\${languages.ada.version}Packages.gpr2 or null";
          description = "The GPR2 package to use (if available).";
        };
      };

      xmlada = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = false;
          description = "Enable XMLAda XML processing library.";
        };
        package = lib.mkOption {
          type = lib.types.nullOr lib.types.package;
          default = if (builtins.hasAttr "xmlada" gnatPackages) then gnatPackages.xmlada else null;
          defaultText = lib.literalExpression "gnat\${languages.ada.version}Packages.xmlada or null";
          description = "The XMLAda package to use (if available).";
        };
      };

      aws = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = false;
          description = "Enable Ada Web Server (AWS) library.";
        };
        package = lib.mkOption {
          type = lib.types.nullOr lib.types.package;
          default = if (builtins.hasAttr "aws" gnatPackages) then gnatPackages.aws else null;
          defaultText = lib.literalExpression "gnat\${languages.ada.version}Packages.aws or null";
          description = "The AWS package to use (if available).";
        };
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = false;
          description = ''
            Enable ada-language-server language server.
          '';
        };
        package = lib.mkOption {
          type = lib.types.nullOr lib.types.package;
          default = if (builtins.hasAttr "ada_language_server" pkgs) then pkgs.ada_language_server else null;
          defaultText = lib.literalExpression "pkgs.ada_language_server or null";
          description = "The Ada Language Server package to use (if available).";
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
          description = "The GDB package to use.";
        };
      };
    };

    extraPackages = lib.mkOption {
      type = lib.types.listOf lib.types.package;
      default = [ ];
      description = "Additional Ada packages to include in the environment.";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ] ++ lib.optionals cfg.dev.enable (
      lib.optional (cfg.dev.gprbuild.enable) cfg.dev.gprbuild.package ++
        lib.optional (cfg.dev.gnatcoll.enable) cfg.dev.gnatcoll.package ++
        lib.optionals (cfg.dev.gnatcoll-bindings.enable) cfg.dev.gnatcoll-bindings.packages ++
        lib.optional (cfg.dev.spark.enable && cfg.dev.spark.package != null) cfg.dev.spark.package ++
        lib.optional (cfg.dev.gpr2.enable && cfg.dev.gpr2.package != null) cfg.dev.gpr2.package ++
        lib.optional (cfg.dev.xmlada.enable && cfg.dev.xmlada.package != null) cfg.dev.xmlada.package ++
        lib.optional (cfg.dev.aws.enable && cfg.dev.aws.package != null) cfg.dev.aws.package ++
        lib.optional (cfg.dev.lsp.enable && cfg.dev.lsp.package != null) cfg.dev.lsp.package ++
        lib.optional (cfg.dev.debugger.enable && lib.meta.availableOn pkgs.stdenv.hostPlatform pkgs.gdb) cfg.dev.debugger.package
    ) ++ cfg.extraPackages;

    # Set environment variables for Ada development
    env = {
      # Set ADA_PROJECT_PATH to help GPR tools find project files
      ADA_PROJECT_PATH = lib.concatStringsSep ":" (
        lib.flatten [
          # GNATCOLL project files
          (lib.optional cfg.dev.gnatcoll.enable "${cfg.dev.gnatcoll.package}/share/gpr")
          # Additional GNATCOLL bindings project files
          (lib.optionals cfg.dev.gnatcoll-bindings.enable
            (map (pkg: "${pkg}/share/gpr") cfg.dev.gnatcoll-bindings.packages))
          # GPR2 project files
          (lib.optional (cfg.dev.gpr2.enable && cfg.dev.gpr2.package != null)
            "${cfg.dev.gpr2.package}/share/gpr")
          # XMLAda project files
          (lib.optional (cfg.dev.xmlada.enable && cfg.dev.xmlada.package != null)
            "${cfg.dev.xmlada.package}/lib/gnat")
          # AWS project files
          (lib.optional (cfg.dev.aws.enable && cfg.dev.aws.package != null)
            "${cfg.dev.aws.package}/lib/gnat")
          # Extra packages project files
          (map (pkg: "${pkg}/share/gpr") cfg.extraPackages)
        ]
      );

      # Set GPR_PROJECT_PATH as an alias (some tools prefer this)
      GPR_PROJECT_PATH = config.env.ADA_PROJECT_PATH;
    };

    # Enable C toolchain as Ada typically needs it for linking
    languages.c.enable = lib.mkDefault true;

    enterShell = ''
      echo "Ada development environment activated!"
      echo "GNAT version: ${cfg.version}"
      echo "Compiler: $(command -v gnat 2>/dev/null && gnat --version | head -1 || echo 'gnat command not found')"
      ${lib.optionalString cfg.dev.gprbuild.enable ''
        echo "GPRbuild: $(command -v gprbuild 2>/dev/null && gprbuild --version | head -1 || echo 'gprbuild not found')"
      ''}
      ${lib.optionalString (cfg.dev.spark.enable && cfg.dev.spark.package != null) ''
        echo "SPARK: $(command -v gnatprove 2>/dev/null && gnatprove --version | head -1 || echo 'gnatprove not found')"
      ''}
      ${lib.optionalString (cfg.dev.lsp.enable && cfg.dev.lsp.package != null) ''
        echo "Ada Language Server: $(command -v ada_language_server 2>/dev/null && echo 'available' || echo 'not found')"
      ''}
      ${lib.optionalString (config.env.ADA_PROJECT_PATH != "") ''
        echo "ADA_PROJECT_PATH configured with ${toString (lib.length (lib.splitString ":" config.env.ADA_PROJECT_PATH))} entries"
      ''}
    '';
  };
}
