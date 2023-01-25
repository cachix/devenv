{ pkgs, config, lib, ... }:

let
  cfg = config.languages.dotnet;
in
{
  options.languages.dotnet = {
    enable = lib.mkEnableOption "tools for .NET development";
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      dotnet-sdk
    ];
  };
}
