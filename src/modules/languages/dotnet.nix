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

    version = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = ''
        The .NET SDK version to use, as `MAJOR.MINOR`.
        This automatically sets the `languages.dotnet.package` from nixpkgs' `dotnetCorePackages`.
      '';
      example = "9.0";
    };

    lsp = {
      enable = lib.mkEnableOption ".NET Language Server" // {
        # csharp-ls crashes on aarch64-darwin due to missing code signatures
        # https://github.com/razzmatazz/csharp-language-server/issues/211
        default = lib.meta.availableOn pkgs.stdenv.hostPlatform pkgs.csharp-ls;
        defaultText = lib.literalMD "`true` if csharp-ls is available on the host platform, `false` otherwise";
      };
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.csharp-ls;
        defaultText = lib.literalExpression "pkgs.csharp-ls";
        description = "The .NET language server package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    languages.dotnet.package = lib.mkIf (cfg.version != null) (
      pkgs.dotnetCorePackages."sdk_${lib.replaceStrings [ "." ] [ "_" ] cfg.version}"
        or (throw "Unsupported .NET SDK version '${cfg.version}', see nixpkgs' dotnetCorePackages for supported versions")
    );

    packages = [
      cfg.package
    ] ++ lib.optional cfg.lsp.enable cfg.lsp.package;

    env.DOTNET_ROOT = "${
        if lib.hasAttr "unwrapped" cfg.package
        then cfg.package.unwrapped
        else cfg.package
    }/share/dotnet";
  };
}
