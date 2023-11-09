{ pkgs, config, lib, ... }:

let cfg = config.languages.idris;
in {
  options.languages.idris = {
    enable = lib.mkEnableOption "tools for Idris development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.idris2;
      defaultText = "pkgs.idris2";
      description = ''
        The Idris package to use.
      '';
      example = "pkgs.idris";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      cfg.package
    ];
  };
}
