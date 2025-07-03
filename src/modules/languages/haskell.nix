{ pkgs, config, lib, ... }:

let
  cfg = config.languages.haskell;
  # ghc.version with removed dots
  ghcVersion = lib.replaceStrings [ "." ] [ "" ] cfg.package.version;
in
{
  options.languages.haskell = {
    enable = lib.mkEnableOption "tools for Haskell development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.ghc;
      defaultText = lib.literalExpression "pkgs.ghc";
      description = ''
        Haskell compiler to use.
      '';
    };

    stack = lib.mkOption {
      type = lib.types.nullOr lib.types.package;
      default = pkgs.stack;
      defaultText = lib.literalExpression "pkgs.stack";
      description = ''
        Haskell stack to use.
      '';
    };

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable Haskell development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable haskell-language-server language server.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.haskell-language-server.override
            {
              supportedGhcVersions = [ ghcVersion ];
            };
          defaultText = lib.literalExpression ''
            pkgs.haskell-language-server.override {
              supportedGhcVersions = [ ghcVersion ];
            }
          '';
          description = ''
            The haskell-language-server package to use.
          '';
        };
      };

      formatter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable ormolu formatter.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.haskellPackages.ormolu;
          defaultText = lib.literalExpression "pkgs.haskellPackages.ormolu";
          description = "The ormolu package to use.";
        };
      };

      linter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable hlint linter.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.haskellPackages.hlint;
          defaultText = lib.literalExpression "pkgs.haskellPackages.hlint";
          description = "The hlint package to use.";
        };
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      cfg.package
      cabal-install
      zlib
      hpack
    ]
    ++ (lib.optional (cfg.stack != null) cfg.stack)
    ++ lib.optionals cfg.dev.enable (
      lib.optional cfg.dev.lsp.enable cfg.dev.lsp.package ++
        lib.optional cfg.dev.formatter.enable cfg.dev.formatter.package ++
        lib.optional cfg.dev.linter.enable cfg.dev.linter.package
    );
  };
}
