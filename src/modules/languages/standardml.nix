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
      defaultText = "pkgs.mlton";
      description = ''
        The Standard ML package to use.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      cfg.package
      millet
      smlfmt
    ];
  };
}
