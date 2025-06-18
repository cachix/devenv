{ pkgs
, config
, lib
, ...
}:
let
  cfg = config.languages.r;
  descriptionFile = import ./descriptionFile.nix;
in
{
  options.languages.r = {
    enable = lib.mkEnableOption "tools for R development";
    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.R;
      defaultText = lib.literalExpression "pkgs.R";
      description = "The R package to use.";
    };
    radian = {
      enable = lib.mkEnableOption "a 21 century R console";
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.radianWrapper;
        defaultText = lib.literalExpression "pkgs.radianWrapper";
        description = "The radian package to use.";
      };
    };
    descriptionFile = {
      path = lib.mkOption {
        type = lib.types.pathInStore;
        description = "Path to DESCRIPTION file";
      };
      installPackages = {
        enable = lib.mkEnableOption "installation of R packages listed in DESCRIPTION file";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages =
      [ cfg.package ]
      ++ lib.lists.optional cfg.radian.enable cfg.radian.package
      ++ lib.lists.optionals cfg.descriptionFile.installPackages.enable (
        descriptionFile.getRPackages pkgs (builtins.readFile cfg.descriptionFile.path)
      );
  };
}
