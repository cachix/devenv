{ pkgs, config, lib, ... }:

let
  cfg = config.languages.unison;
in
{
  options.languages.unison = {
    enable = lib.mkEnableOption "tools for Unison development";

    package = lib.mkOption {
      type = lib.types.package;
      description = "Which package of Unison to use";
      default = pkgs.unison-ucm;
      defaultText = lib.literalExpression "pkgs.unison-ucm";
    };

    lsp = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = ''
          Enable Unison language server support.
          The language server is included with the main Unison package.
        '';
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ];
  };
}
