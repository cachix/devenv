{ pkgs
, config
, lib
, ...
}:
let
  cfg = config.languages.d;
in
{
  options.languages.d = {
    enable = lib.mkEnableOption "tools for D language development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.ldc;
      defaultText = lib.literalExpression "pkgs.ldc";
      description = "The D compiler package to use (LDC by default).";
    };

    lsp = {
      enable = lib.mkEnableOption "D language server." // { default = true; };
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.serve-d;
        defaultText = lib.literalExpression "pkgs.serve-d";
        description = "The D language server package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages =
      [
        cfg.package
        pkgs.dub
      ]
      ++ lib.optional cfg.lsp.enable cfg.lsp.package;
  };
}
