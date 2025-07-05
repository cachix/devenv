{ pkgs, config, lib, ... }:

let
  cfg = config.languages.ocaml;
in
{
  options.languages.ocaml = {
    enable = lib.mkEnableOption "tools for OCaml development";

    packages = lib.mkOption
      {
        type = lib.types.attrs;
        description = "The package set of OCaml to use";
        default = pkgs.ocaml-ng.ocamlPackages;
        defaultText = lib.literalExpression "pkgs.ocaml-ng.ocamlPackages_4_12";
      };

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable OCaml development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable OCaml language server (ocaml-lsp).";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = cfg.packages.ocaml-lsp;
          defaultText = lib.literalExpression "cfg.packages.ocaml-lsp";
          description = "The ocaml-lsp package to use.";
        };
      };

      formatter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable OCaml formatter (ocamlformat).";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.ocamlformat;
          defaultText = lib.literalExpression "pkgs.ocamlformat";
          description = "The ocamlformat package to use.";
        };
      };

      tools = {
        merlin = {
          enable = lib.mkOption {
            type = lib.types.bool;
            default = true;
            description = "Enable Merlin editor service.";
          };
          package = lib.mkOption {
            type = lib.types.package;
            default = cfg.packages.merlin;
            defaultText = lib.literalExpression "cfg.packages.merlin";
            description = "The merlin package to use.";
          };
        };

        utop = {
          enable = lib.mkOption {
            type = lib.types.bool;
            default = true;
            description = "Enable UTop REPL.";
          };
          package = lib.mkOption {
            type = lib.types.package;
            default = cfg.packages.utop;
            defaultText = lib.literalExpression "cfg.packages.utop";
            description = "The utop package to use.";
          };
        };

        ocp-indent = {
          enable = lib.mkOption {
            type = lib.types.bool;
            default = true;
            description = "Enable ocp-indent indenter.";
          };
          package = lib.mkOption {
            type = lib.types.package;
            default = cfg.packages.ocp-indent;
            defaultText = lib.literalExpression "cfg.packages.ocp-indent";
            description = "The ocp-indent package to use.";
          };
        };

        odoc = {
          enable = lib.mkOption {
            type = lib.types.bool;
            default = true;
            description = "Enable odoc documentation generator.";
          };
          package = lib.mkOption {
            type = lib.types.package;
            default = cfg.packages.odoc;
            defaultText = lib.literalExpression "cfg.packages.odoc";
            description = "The odoc package to use.";
          };
        };
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      # Core tools
      cfg.packages.ocaml
      cfg.packages.dune_3
      cfg.packages.findlib
    ] ++ lib.optionals cfg.dev.enable (
      lib.optional cfg.dev.lsp.enable cfg.dev.lsp.package ++
        lib.optional cfg.dev.formatter.enable cfg.dev.formatter.package ++
        lib.optional cfg.dev.tools.merlin.enable cfg.dev.tools.merlin.package ++
        lib.optional cfg.dev.tools.utop.enable cfg.dev.tools.utop.package ++
        lib.optional cfg.dev.tools.ocp-indent.enable cfg.dev.tools.ocp-indent.package ++
        lib.optional cfg.dev.tools.odoc.enable cfg.dev.tools.odoc.package
    );
  };
}
