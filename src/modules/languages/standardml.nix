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

    lsp = {
      enable = lib.mkEnableOption "Standard ML Language Server" // { default = true; };

      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.millet;
        defaultText = lib.literalExpression "pkgs.millet";
        description = "The Standard ML language server package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
      pkgs.smlfmt
    ] ++ lib.optional cfg.lsp.enable cfg.lsp.package;
  };
}
