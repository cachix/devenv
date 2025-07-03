{ pkgs, config, lib, ... }:

let
  cfg = config.languages.erlang;
  rebar3 = pkgs.rebar3.overrideAttrs (oldAttrs: {
    buildInputs = [ cfg.package ];
  });
in
{
  options.languages.erlang = {
    enable = lib.mkEnableOption "tools for Erlang development";

    package = lib.mkOption {
      type = lib.types.package;
      description = "Which package of Erlang to use.";
      default = pkgs.erlang_27;
      defaultText = lib.literalExpression "pkgs.erlang";
    };

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable Erlang development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable erlang-ls language server.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.erlang-ls;
          defaultText = lib.literalExpression "pkgs.erlang-ls";
          description = "The erlang-ls package to use.";
        };
      };
    };
  };

  config = lib.mkIf cfg.enable
    {
      packages = [
        cfg.package
        rebar3
      ] ++ lib.optionals cfg.dev.enable (
        lib.optional (cfg.dev.lsp.enable) cfg.dev.lsp.package
      );
    };
}
