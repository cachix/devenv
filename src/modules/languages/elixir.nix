{ pkgs, config, lib, ... }:

let
  cfg = config.languages.elixir;
in
{
  options.languages.elixir = {
    enable = lib.mkEnableOption "tools for Elixir development";

    package = lib.mkOption {
      type = lib.types.package;
      description = "Which package of Elixir to use.";
      default = pkgs.elixir;
      defaultText = lib.literalExpression "pkgs.elixir";
    };

    languageServer = {
      elixir_ls = lib.mkOption {
        type = lib.types.bool;
        default = true;
        defaultText = "true";
        description = ''
          Enable the ElixirLS language server (https://github.com/elixir-lsp/elixir-ls).
        '';
      };

      lexical = lib.mkOption {
        type = lib.types.bool;
        default = true;
        defaultText = "true";
        description = ''
          Enable the Lexical language server (https://github.com/lexical-lsp/lexical).
        '';
      };
    };
  };

  config = lib.mkIf cfg.enable
    {
      packages = with pkgs; [
        cfg.package
      ]
      ++ lib.optional cfg.languageServer.elixir_ls elixir-ls
      ++ lib.optional cfg.languageServer.lexical lexical;
    };
}
