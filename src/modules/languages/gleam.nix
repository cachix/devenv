{ pkgs, config, lib, ... }:

let
  cfg = config.languages.gleam;
in
{
  options.languages.gleam = {
    enable = lib.mkEnableOption "tools for Gleam development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.gleam;
      description = "The Gleam package to use.";
      defaultText = lib.literalExpression "pkgs.gleam";
    };

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable Gleam development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable gleam language server.";
        };
      };

      formatter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable gleam formatter.";
        };
      };
    };
  };

  config = lib.mkIf cfg.enable {
    languages.erlang.enable = true;

    packages = [
      cfg.package
    ];
  };
}
