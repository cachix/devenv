{ pkgs, config, lib, ... }:

let
  cfg = config.languages.dotnet;
in
{
  options.languages.dotnet = {
    enable = lib.mkEnableOption "tools for .NET development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.dotnetCorePackages.sdk_7_0;
      defaultText = lib.literalExpression "pkgs.dotnet-sdk";
      description = "The .NET SDK package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      cfg.package
    ];

    env = {
      DOTNET_ROOT = "${cfg.package}";
      DOTNET_CLI_TELEMETRY_OPTOUT = 1;
      DOTNET_SKIP_FIRST_TIME_EXPERIENCE = true;
      LD_LIBRARY_PATH = "$LD_LIBRARY_PATH:${lib.makeLibraryPath [ pkgs.icu ]}";
    };
  };
}
