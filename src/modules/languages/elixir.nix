{ pkgs, config, lib, ... }:

let
  cfg = config.languages.elixir;
in
{
  options.languages.elixir = {
    enable = lib.mkEnableOption "tools for Elixir development";

    package = lib.mkOption {
      type = lib.types.package;
      description = "Which Elixir package to use.";
      default = pkgs.elixir;
      defaultText = lib.literalExpression "pkgs.elixir";
    };

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable Elixir development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable elixir-ls language server.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.elixir_ls;
          defaultText = lib.literalExpression "pkgs.elixir_ls";
          description = "The elixir-ls package to use.";
        };
      };

      linter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable credo linter.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.beamPackages.credo;
          defaultText = lib.literalExpression "pkgs.beamPackages.credo";
          description = "The credo package to use.";
        };
      };

      dialyzer = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable dialyxir static analyzer.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.beamPackages.dialyxir;
          defaultText = lib.literalExpression "pkgs.beamPackages.dialyxir";
          description = "The dialyxir package to use.";
        };
      };
    };
  };

  config = lib.mkIf cfg.enable
    {
      git-hooks.hooks = {
        credo.package = cfg.package;
        dialyzer.package = cfg.package;
        mix-format.package = cfg.package;
        mix-test.package = cfg.package;
      };

      packages = [
        cfg.package
      ] ++ lib.optionals cfg.dev.enable (
        lib.optional (cfg.dev.lsp.enable) cfg.dev.lsp.package ++
          lib.optional (cfg.dev.linter.enable) cfg.dev.linter.package ++
          lib.optional (cfg.dev.dialyzer.enable) cfg.dev.dialyzer.package
      );
    };
}
