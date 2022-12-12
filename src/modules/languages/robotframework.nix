{ pkgs, config, lib, ... }:

let
  cfg = config.languages.robotframework;
in
{
  options.languages.robotframework = {
    enable = lib.mkEnableOption "Enable tools for Robot Framework development.";

    python = lib.mkOption {
      type = lib.types.package;
      default = pkgs.python3;
      defaultText = "pkgs.python3";
      description = "The Python package to use.";
    };

  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      (cfg.python.withPackages (ps: [
        ps.robotframework
      ]))
    ];
  };
}
