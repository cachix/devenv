{ pkgs, config, lib, ... }:

let
  cfg = config.languages.erlang;
  # There's no override available
  rebar3 = pkgs.rebar3.overrideAttrs (_: {
    buildInputs = [ cfg.package ];
  });
in
{
  options.languages.erlang = {
    enable = lib.mkEnableOption "tools for Erlang development";

    package = lib.mkOption {
      type = lib.types.package;
      description = "Which Erlang package to use.";
      default = pkgs.erlang;
      defaultText = lib.literalExpression "pkgs.erlang";
    };

    lsp = {
      enable = lib.mkEnableOption "Erlang Language Server" // { default = true; };

      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.erlang-language-platform;
        defaultText = lib.literalExpression "pkgs.erlang-language-platform";
        description = "The Erlang language server package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
      rebar3
    ] ++ lib.optional cfg.lsp.enable cfg.lsp.package;
  };
}
