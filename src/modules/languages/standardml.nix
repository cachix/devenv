{ pkgs, config, lib, ... }:

let
  cfg = config.languages.standardml;
in
{
  options.languages.standardml = {
    enable = lib.mkEnableOption "tools for Standard ML development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.mlton;
      defaultText = lib.literalExpression "pkgs.mlton";
      description = ''
        The Standard ML package to use.
      '';
    };

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable Standard ML development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable Standard ML language server (millet).";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.millet;
          defaultText = lib.literalExpression "pkgs.millet";
          description = "The millet package to use.";
        };
      };

      formatter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable Standard ML formatter (smlfmt).";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.smlfmt;
          defaultText = lib.literalExpression "pkgs.smlfmt";
          description = "The smlfmt package to use.";
        };
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ] ++ lib.optionals cfg.dev.enable (
      lib.optional cfg.dev.lsp.enable cfg.dev.lsp.package ++
        lib.optional cfg.dev.formatter.enable cfg.dev.formatter.package
    );
  };
}
