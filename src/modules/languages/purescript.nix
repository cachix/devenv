{ pkgs, config, lib, ... }:

let
  cfg = config.languages.purescript;
  # supported via rosetta
  supportAarch64Darwin = package: package.overrideAttrs (attrs: {
    meta = attrs.meta // {
      platforms = lib.platforms.linux ++ lib.platforms.darwin;
    };
  });
in
{
  options.languages.purescript = {
    enable = lib.mkEnableOption "tools for PureScript development";

    package = lib.mkOption {
      type = lib.types.package;
      default = (supportAarch64Darwin pkgs.purescript);
      defaultText = lib.literalExpression "pkgs.purescript";
      description = "The PureScript package to use.";
    };

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable PureScript development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable PureScript language server.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.nodePackages.purescript-language-server;
          defaultText = lib.literalExpression "pkgs.nodePackages.purescript-language-server";
          description = "The purescript-language-server package to use.";
        };
      };

      formatter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable purs-tidy formatter.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.nodePackages.purs-tidy;
          defaultText = lib.literalExpression "pkgs.nodePackages.purs-tidy";
          description = "The purs-tidy package to use.";
        };
      };

      tools = {
        psa = {
          enable = lib.mkOption {
            type = lib.types.bool;
            default = true;
            description = "Enable PureScript Assistant (psa).";
          };
          package = lib.mkOption {
            type = lib.types.package;
            default = pkgs.purescript-psa;
            defaultText = lib.literalExpression "pkgs.purescript-psa";
            description = "The purescript-psa package to use.";
          };
        };
      };
    };

  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
      pkgs.spago
      (supportAarch64Darwin pkgs.psc-package)
    ] ++ lib.optionals cfg.dev.enable (
      lib.optional cfg.dev.lsp.enable cfg.dev.lsp.package ++
        lib.optional cfg.dev.formatter.enable cfg.dev.formatter.package ++
        lib.optional cfg.dev.tools.psa.enable cfg.dev.tools.psa.package
    );
  };
}
