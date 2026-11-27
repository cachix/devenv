{ pkgs
, config
, lib
, ...
}:

let
  cfg = config.languages.coffeescript;
in
{
  options.languages.coffeescript = {
    enable = lib.mkEnableOption "tools for CoffeeScript development.";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.coffeescript;
      defaultText = lib.literalExpression "pkgs.coffeescript";
      description = "The CoffeeScript package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ];
  };
}
