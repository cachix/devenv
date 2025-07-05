{ pkgs, config, lib, ... }:

let
  cfg = config.languages.go;
in
{
  options.languages.go =
    let
      # Override the buildGoModule function to use the specified Go package.
      buildGoModule = pkgs.buildGoModule.override { go = cfg.package; };
      buildWithSpecificGo = pkg:
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

      dev = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable Go development tools.";
        };

        lsp = {
          enable = lib.mkOption {
            type = lib.types.bool;
            default = true;
            description = "Enable gopls language server.";
          };
          package = lib.mkOption {
            type = lib.types.package;
            default = buildWithSpecificGo pkgs.gopls;
            defaultText = lib.literalExpression "buildWithSpecificGo pkgs.gopls";
            description = "The gopls package to use.";
          };
        };

        debugger = {
          enable = lib.mkOption {
            type = lib.types.bool;
            default = true;
            description = "Enable delve debugger.";
          };
          package = lib.mkOption {
            type = lib.types.package;
            default = buildWithSpecificGo pkgs.delve;
            defaultText = lib.literalExpression "buildWithSpecificGo pkgs.delve";
            description = "The delve package to use.";
          };
        };

        gotools = {
          enable = lib.mkOption {
            type = lib.types.bool;
            default = true;
            description = "Enable gotools development tools.";
          };
          package = lib.mkOption {
            type = lib.types.package;
            default = buildWithSpecificGo pkgs.gotools;
            defaultText = lib.literalExpression "buildWithSpecificGo pkgs.gotools";
            description = "The gotools package to use.";
          };
        };

        gomodifytags = {
          enable = lib.mkOption {
            type = lib.types.bool;
            default = true;
            description = "Enable gomodifytags struct tag tool.";
          };
          package = lib.mkOption {
            type = lib.types.package;
            default = buildWithSpecificGo pkgs.gomodifytags;
            defaultText = lib.literalExpression "buildWithSpecificGo pkgs.gomodifytags";
            description = "The gomodifytags package to use.";
          };
        };

        impl = {
          enable = lib.mkOption {
            type = lib.types.bool;
            default = true;
            description = "Enable impl interface implementation generator.";
          };
          package = lib.mkOption {
            type = lib.types.package;
            default = buildWithSpecificGo pkgs.impl;
            defaultText = lib.literalExpression "buildWithSpecificGo pkgs.impl";
            description = "The impl package to use.";
          };
        };

        go-tools = {
          enable = lib.mkOption {
            type = lib.types.bool;
            default = true;
            description = "Enable go-tools static analysis tools.";
          };
          package = lib.mkOption {
            type = lib.types.package;
            default = buildWithSpecificGo pkgs.go-tools;
            defaultText = lib.literalExpression "buildWithSpecificGo pkgs.go-tools";
            description = "The go-tools package to use.";
          };
        };

        gotests = {
          enable = lib.mkOption {
            type = lib.types.bool;
            default = true;
            description = "Enable gotests test generator.";
          };
          package = lib.mkOption {
            type = lib.types.package;
            default = buildWithSpecificGo pkgs.gotests;
            defaultText = lib.literalExpression "buildWithSpecificGo pkgs.gotests";
            description = "The gotests package to use.";
          };
        };

        iferr = {
          enable = lib.mkOption {
            type = lib.types.bool;
            default = true;
            description = "Enable iferr error handling generator.";
          };
          package = lib.mkOption {
            type = lib.types.package;
            default = buildWithSpecificGo pkgs.iferr;
            defaultText = lib.literalExpression "buildWithSpecificGo pkgs.iferr";
            description = "The iferr package to use.";
          };
        };
      };
    };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ] ++ lib.optionals cfg.dev.enable (
      lib.optional (cfg.dev.lsp.enable) cfg.dev.lsp.package ++
        lib.optional (cfg.dev.debugger.enable) cfg.dev.debugger.package ++
        lib.optional (cfg.dev.gotools.enable) cfg.dev.gotools.package ++
        lib.optional (cfg.dev.gomodifytags.enable) cfg.dev.gomodifytags.package ++
        lib.optional (cfg.dev.impl.enable) cfg.dev.impl.package ++
        lib.optional (cfg.dev.go-tools.enable) cfg.dev.go-tools.package ++
        lib.optional (cfg.dev.gotests.enable) cfg.dev.gotests.package ++
        lib.optional (cfg.dev.iferr.enable) cfg.dev.iferr.package
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
