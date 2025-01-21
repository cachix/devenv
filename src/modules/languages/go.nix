{ pkgs, config, lib, ... }:

let
  cfg = config.languages.go;

  # Override the buildGoModule function to use the specified Go package.
  buildGoModule = pkgs.buildGoModule.override { go = cfg.package; };
  buildWithSpecificGo = pkg: pkg.override { inherit buildGoModule; };
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

    enableHardeningWorkaround = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Enable hardening workaround required for Delve debugger (https://github.com/go-delve/delve/issues/3085)";
    };
  };

  config = lib.mkIf cfg.enable {
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
