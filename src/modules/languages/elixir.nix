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

    lsp = {
      enable = lib.mkEnableOption "Elixir Language Server" // { default = true; };

      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.elixir-ls;
        defaultText = lib.literalExpression "pkgs.elixir-ls";
        description = "The Elixir language server package to use.";
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
      ] ++ lib.optional cfg.lsp.enable cfg.lsp.package;
    };
}
