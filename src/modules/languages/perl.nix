{ pkgs, config, lib, ... }:

let
  cfg = config.languages.perl;
in
{
  options.languages.perl = {
    enable = lib.mkEnableOption "tools for Perl development";
    packages = lib.mkOption
      {
        type = lib.types.listOf lib.types.str;
        description = "Perl packages to include";
        default = [ ];
        example = [ "Mojolicious" ];
      };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      (perl.withPackages (p: (with builtins; map
        (pkg: p.${ replaceStrings [ "::" ] [ "" ] pkg })
        cfg.packages)))
    ];
  };
}
