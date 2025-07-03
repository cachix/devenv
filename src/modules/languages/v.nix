{ pkgs, config, lib, ... }:

let
  cfg = config.languages.v;
in
{
  options.languages.v = {
    enable = lib.mkEnableOption "tools for V development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.vlang;
      defaultText = lib.literalExpression "pkgs.vlang";
      description = "The V package to use.";
    };

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable V development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable V language server.";
        };
        # v-analyzer is available in V installation itself
        # V includes its own language server called v-ls
        package = lib.mkOption {
          type = lib.types.package;
          default = cfg.package;
          defaultText = lib.literalExpression "config.languages.v.package";
          description = "The V package to use (includes v-ls language server).";
        };
      };

      formatter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable V formatter (v fmt built into V compiler).";
        };
        # v fmt is built into the V compiler, so no separate package needed
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ] ++ lib.optionals cfg.dev.enable (
      lib.optional cfg.dev.lsp.enable cfg.dev.lsp.package
    );
  };
}
