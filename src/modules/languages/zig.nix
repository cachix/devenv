{ pkgs
, config
, lib
, ...
}:

let
  cfg = config.languages.zig;
in
{
  options.languages.zig = {
    enable = lib.mkEnableOption "tools for Zig development";

    package = lib.mkOption {
      type = lib.types.package;
      description = "Which package of Zig to use.";
      default = pkgs.zig;
      defaultText = lib.literalExpression "pkgs.zig";
    };

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable Zig development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable Zig language server (zls).";
        };
        package = lib.mkOption {
          type = lib.types.package;
          description = "Which package of zls to use.";
          default = pkgs.zls;
          defaultText = lib.literalExpression "pkgs.zls";
        };
      };

      formatter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable zig formatter.";
        };
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ] ++ lib.optionals cfg.dev.enable (
      lib.optional (cfg.dev.lsp.enable) cfg.dev.lsp.package
    );
  };
}
