{ pkgs, config, lib, ... }:

let
  cfg = config.languages.go;

  go-overlay = config.lib.getInput {
    name = "go-overlay";
    url = "github:purpleclay/go-overlay";
    attribute = "languages.go.version";
    follows = [ "nixpkgs" ];
  };

  go-bin = go-overlay.lib.mkGoBin pkgs;

  # Override the buildGoModule function to use the specified Go package.
  buildGoModule = pkgs.buildGoModule.override { go = cfg.package; };
  # A helper function to rebuild a package with the specific Go version.
  # It expects the package to have a `buildGo*Module` argument in its override function.
  # This will override multiple buildGo*Module arguments if they exist.
  buildWithSpecificGo = pkg:
    let
      overrideArgs = lib.functionArgs pkg.override;
      goModuleArgs = lib.filterAttrs (name: _: lib.match "buildGo.*Module" name != null) overrideArgs;
      goModuleOverrides = lib.mapAttrs (_: _: buildGoModule) goModuleArgs;
    in
    if goModuleOverrides != { } then
      pkg.override goModuleOverrides
    else
      throw ''
        `languages.go` failed to override the Go version for ${pkg.pname or "unknown"}.
        Expected to find a `buildGo*Module` argument in its override function.

        Found: ${toString (lib.attrNames overrideArgs)}
      '';
in
{
  options.languages.go = {
    enable = lib.mkEnableOption "tools for Go development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.go;
      defaultText = lib.literalExpression "pkgs.go";
      description = "The Go package to use.";
    };

    version = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = ''
        The Go version to use.
        This automatically sets the `languages.go.package` using [go-overlay](https://github.com/purpleclay/go-overlay).
      '';
      example = "1.22.0";
    };

    enableHardeningWorkaround = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Enable hardening workaround required for Delve debugger (https://github.com/go-delve/delve/issues/3085)";
    };
  };

  config = lib.mkIf cfg.enable {
    languages.go.package = lib.mkIf (cfg.version != null) (
      go-bin.versions.${cfg.version}
        or (throw "Unsupported Go version '${cfg.version}', see https://github.com/purpleclay/go-overlay for supported versions")
    );

    packages = [
      cfg.package

      # Required by vscode-go
      (buildWithSpecificGo pkgs.delve)

      # vscode-go expects all tool compiled with the same used go version, see: https://github.com/golang/vscode-go/blob/72249dc940e5b6ec97b08e6690a5f042644e2bb5/src/goInstallTools.ts#L721
      (buildWithSpecificGo pkgs.gotools)
      (buildWithSpecificGo pkgs.gomodifytags)
      (buildWithSpecificGo pkgs.impl)
      (buildWithSpecificGo pkgs.go-tools)
      (buildWithSpecificGo pkgs.gopls)
      (buildWithSpecificGo pkgs.gotests)

      # Required by vim-go
      (buildWithSpecificGo pkgs.iferr)
    ];

    hardeningDisable = lib.optional (cfg.enableHardeningWorkaround) "fortify";

    env.GOROOT = cfg.package + "/share/go/";
    env.GOPATH = config.env.DEVENV_STATE + "/go";
    env.GOTOOLCHAIN = "local";

    enterShell = ''
      export PATH=$GOPATH/bin:$PATH
    '';
  };
}
