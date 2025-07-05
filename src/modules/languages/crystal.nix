{ pkgs, config, lib, ... }:

let
  cfg = config.languages.crystal;
in
{
  options.languages.crystal = {
    enable = lib.mkEnableOption "Enable tools for Crystal development.";

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable Crystal development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable crystalline language server.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.crystalline;
          defaultText = lib.literalExpression "pkgs.crystalline";
          description = "The crystalline package to use.";
        };
      };

      formatter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable crystal formatter.";
        };
      };
    };
  };

  config = lib.mkIf cfg.enable {
    # enable compiler tooling by default to expose things like cc
    languages.c.enable = lib.mkDefault true;

    packages = [
      pkgs.crystal
      pkgs.shards
    ] ++ lib.optionals cfg.dev.enable (
      lib.optional cfg.dev.lsp.enable cfg.dev.lsp.package
    );
  };
}
