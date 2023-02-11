{ pkgs, config, lib, ... }:

let
  cfg = config.languages.go;
in
{
  options.languages.go = {
    enable = lib.mkEnableOption "tools for Go development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.go;
      defaultText = lib.literalExpression "pkgs.go";
      description = "The Go package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      cfg.package
      gotools
    ];

    env.GOROOT = cfg.package + "/share/go/";
  };
}
