{ pkgs
, config
, lib
, ...
}:

let
  cfg = config.languages.go;

  # Override the buildGoModule function to use the specified Go package.
  buildGoModule = pkgs.buildGoModule.override { go = cfg.package; };
  buildWithSpecificGo =
    pkg:
    let
      overrideArgs = lib.functionArgs pkg.override;
    in
    if builtins.hasAttr "buildGoModule" overrideArgs then
      pkg.override { inherit buildGoModule; }
    else if builtins.hasAttr "buildGoLatestModule" overrideArgs then
      pkg.override { buildGoLatestModule = buildGoModule; }
    else
      throw "Package ${pkg.pname or "unknown"} does not accept buildGoModule or buildGoLatestModule arguments";
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

    tools = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = ''
          Enable Go tools (lsp, debugger & various linter) which
          must be built with the same Go `package`.
        '';
      };

      # LSP and other tools (and vs-code) should be compiled with the same Go version.
      # see: https://github.com/golang/vscode-go/blob/72249dc940e5b6ec97b08e6690a5f042644e2bb5/src/goInstallTools.ts#L721
      # see: https://github.com/golang/tools/blob/master/gopls/README.md
      packages = lib.mkOption {
        type = lib.types.nullOr (lib.types.listOf lib.types.package);
        example = lib.literalExpression "[ pkgs.gopls ]";
        default = null;
        description = ''
          Go packages which need to be built with the chosen Go package.
          If `null`, then `defaultPackages` will be used.
        '';
      };

      packagesDefault = lib.mkOption {
        type = lib.types.listOf lib.types.package;
        example = lib.literalExpression "[ pkgs.gopls ]";
        default = [
          # Debugger,
          pkgs.delve
          # LSP,
          pkgs.gopls
          pkgs.gotools
          pkgs.gomodifytags
          pkgs.impl
          pkgs.go-tools
          pkgs.golines
          pkgs.gotests
          pkgs.iferr
        ];
        description = ''
          Packages which are used for the `packages` option if its `null`.
        '';
      };
    };

    enableHardeningWorkaround = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Enable hardening workaround required for Delve debugger (https://github.com/go-delve/delve/issues/3085)";
    };
  };

  config = lib.mkIf cfg.enable {
    packages =
      [
        cfg.package
      ]
      ++ lib.optionals (cfg.tools.enable) (
        lib.map (p: buildWithSpecificGo p) (cfg.tools.packages or cfg.tools.packagesDefault)
      );

    hardeningDisable = lib.optional (cfg.enableHardeningWorkaround) "fortify";

    env.GOROOT = cfg.package + "/share/go/";
    env.GOPATH = config.env.DEVENV_STATE + "/go";
    env.GOTOOLCHAIN = "local";

    enterShell = ''
      export PATH=$GOPATH/bin:$PATH
    '';
  };
}
