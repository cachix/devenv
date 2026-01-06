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
      example = lib.literalExpression "pkgs.texliveBasic";
      description = "TeX Live package set to use";
    };
    packages = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      example = [ "algorithms" "latexmk" ];
      description = "Extra packages to add to the base TeX Live set";
    };

    lsp = {
      enable = lib.mkEnableOption "TeX Live Language Server" // { default = true; };
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.texlab;
        defaultText = lib.literalExpression "pkgs.texlab";
        description = "The TeX Live language server package to use.";
      };
    };
  };
  config = lib.mkIf cfg.enable {
    packages = [ package ] ++ lib.optional cfg.lsp.enable cfg.lsp.package;
  };
}
