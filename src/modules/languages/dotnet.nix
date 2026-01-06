{ pkgs, config, lib, ... }:

let
  cfg = config.languages.dotnet;
in
{
  options.languages.dotnet = {
    enable = lib.mkEnableOption "tools for .NET development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.dotnetCorePackages.sdk_8_0;
      defaultText = lib.literalExpression "pkgs.dotnet-sdk";
      description = "The .NET SDK package to use.";
    };

    lsp = {
      enable = lib.mkEnableOption ".NET Language Server" // { default = true; };
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.csharp-ls;
        defaultText = lib.literalExpression "pkgs.csharp-ls";
        description = "The .NET language server package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ] ++ lib.optional cfg.lsp.enable cfg.lsp.package;

    env.DOTNET_ROOT = "${
        if lib.hasAttr "unwrapped" cfg.package
        then cfg.package.unwrapped
        else cfg.package
    }/share/dotnet";
    env.LD_LIBRARY_PATH = "$LD_LIBRARY_PATH:${lib.makeLibraryPath [ pkgs.icu ]}";
  };
}
