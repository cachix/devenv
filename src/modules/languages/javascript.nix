{ pkgs, config, lib, ... }:

let
  cfg = config.languages.javascript;
in
{
  options.languages.javascript = {
    enable = lib.mkEnableOption "tools for JavaScript development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.nodejs;
      defaultText = lib.literalExpression "pkgs.nodejs";
      description = "The Node package to use.";
    };

    corepack = {
      enable = lib.mkEnableOption "shims for package managers besides npm";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ] ++ lib.optional cfg.corepack.enable (pkgs.runCommand "corepack-enable" { } ''
      mkdir -p $out/bin
      ${cfg.package}/bin/corepack enable --install-directory $out/bin
    '');
  };
}
