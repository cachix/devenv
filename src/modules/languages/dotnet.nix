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

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable .NET development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable omnisharp-roslyn language server.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.omnisharp-roslyn;
          defaultText = lib.literalExpression "pkgs.omnisharp-roslyn";
          description = "The omnisharp-roslyn package to use.";
        };
      };

      formatter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable dotnet formatter.";
        };
        package = lib.mkOption {
          type = lib.types.nullOr lib.types.package;
          default = null;
          description = "The dotnet-format package to use.";
        };
      };

      debugger = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable netcoredbg debugger.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.netcoredbg;
          defaultText = lib.literalExpression "pkgs.netcoredbg";
          description = "The netcoredbg package to use.";
        };
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ] ++ lib.optionals cfg.dev.enable (
      lib.optional (cfg.dev.lsp.enable) cfg.dev.lsp.package ++
        lib.optional (cfg.dev.debugger.enable) cfg.dev.debugger.package
    );

    env.DOTNET_ROOT = "${
        if lib.hasAttr "unwrapped" cfg.package
        then cfg.package.unwrapped
        else cfg.package
    }/share/dotnet";
    env.LD_LIBRARY_PATH = "$LD_LIBRARY_PATH:${lib.makeLibraryPath [ pkgs.icu ]}";
  };
}
