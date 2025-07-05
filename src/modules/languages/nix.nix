{ pkgs, config, lib, ... }:

let
  cfg = config.languages.nix;
  cachix = lib.getBin config.cachix.package;

  # a bit of indirection to prevent mkShell from overriding the installed Nix
  vulnix = pkgs.buildEnv {
    name = "vulnix";
    paths = [ pkgs.vulnix ];
    pathsToLink = [ "/bin" ];
  };
in
{
  imports = [
    (lib.mkRenamedOptionModule [ "languages" "nix" "lsp" "package" ] [ "languages" "nix" "dev" "lsp" "package" ])
  ];

  options.languages.nix = {
    enable = lib.mkEnableOption "tools for Nix development";

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable Nix development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable Nix language server (nil).";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.nil;
          defaultText = lib.literalExpression "pkgs.nil";
          description = "The nil package to use.";
        };
      };

      formatter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable nixpkgs-fmt formatter.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.nixpkgs-fmt;
          defaultText = lib.literalExpression "pkgs.nixpkgs-fmt";
          description = "The nixpkgs-fmt package to use.";
        };
      };

      linter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable statix linter.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.statix;
          defaultText = lib.literalExpression "pkgs.statix";
          description = "The statix package to use.";
        };
      };

      tools = {
        deadnix = {
          enable = lib.mkOption {
            type = lib.types.bool;
            default = true;
            description = "Enable deadnix.";
          };
          package = lib.mkOption {
            type = lib.types.package;
            default = pkgs.deadnix;
            defaultText = lib.literalExpression "pkgs.deadnix";
            description = "The deadnix package to use.";
          };
        };

        vulnix = {
          enable = lib.mkOption {
            type = lib.types.bool;
            default = true;
            description = "Enable vulnix.";
          };
          package = lib.mkOption {
            type = lib.types.package;
            default = vulnix;
            defaultText = lib.literalExpression "vulnix";
            description = "The vulnix package to use.";
          };
        };
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = lib.optionals cfg.dev.enable
      (
        lib.optional cfg.dev.lsp.enable cfg.dev.lsp.package ++
          lib.optional cfg.dev.formatter.enable cfg.dev.formatter.package ++
          lib.optional cfg.dev.linter.enable cfg.dev.linter.package ++
          lib.optional cfg.dev.tools.deadnix.enable cfg.dev.tools.deadnix.package ++
          lib.optional cfg.dev.tools.vulnix.enable cfg.dev.tools.vulnix.package
      ) ++ (lib.optional config.cachix.enable cachix);
  };
}
