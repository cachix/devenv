{ pkgs, config, lib, ... }:
let
  cfg = config.languages.texlive;

  getPackage = ps: name: ps.${name} or (throw "No such texlive package ${name}");
  package = cfg.base.withPackages (ps: builtins.map (getPackage ps) cfg.packages);
in
{
  options.languages.texlive = {
    enable = lib.mkEnableOption "TeX Live";
    base = lib.mkOption {
      default = pkgs.texliveSmall;
      defaultText = lib.literalExpression "pkgs.texliveSmall";
      description = "TeX Live package set to use";
    };
    packages = lib.mkOption {
      type = lib.types.nonEmptyListOf lib.types.str;
      default = [ "collection-basic" ];
      description = "Packages available to TeX Live";
    };
  };
  config = lib.mkIf cfg.enable {
    packages = [ package ];
  };
}
