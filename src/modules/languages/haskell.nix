{ pkgs, config, lib, ... }:

let
  cfg = config.languages.haskell;

  # Wrapper for stack that configures it to use devenv's GHC
  stackWrapper = pkgs.runCommand "stack-wrapper"
    {
      buildInputs = [ pkgs.makeWrapper ];
    } ''
    mkdir -p $out/bin
    makeWrapper ${cfg.stack.package}/bin/stack $out/bin/stack \
      ${lib.concatMapStringsSep " \\\n      " (arg: "--add-flags \"${arg}\"") cfg.stack.args}
  '';
  # ghc.version with removed dots
  ghcVersion = lib.replaceStrings [ "." ] [ "" ] cfg.package.version;
in
{
  imports = [
    (lib.mkRenamedOptionModule [ "languages" "haskell" "languageServer" ] [ "languages" "haskell" "lsp" "package" ])
  ];

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

    lsp = {
      enable = lib.mkEnableOption "Haskell Language Server" // { default = true; };

      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.haskell-language-server.override
          {
            supportedGhcVersions = [ ghcVersion ];
          };
        defaultText = lib.literalExpression "pkgs.haskell-language-server";
        description = "The Haskell language server package to use.";
      };
    };

    stack = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = '' Whether to enable the Haskell Stack      '';
      };

      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.stack;
        defaultText = lib.literalExpression "pkgs.stack";
        description = ''
          Haskell stack package to use.
        '';
      };

      args = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [ "--no-nix" "--system-ghc" "--no-install-ghc" ];
        defaultText = lib.literalExpression ''[ "--no-nix" "--system-ghc" "--no-install-ghc" ]'';
        description = ''
          Additional arguments to pass to stack.
          By default, stack is configured to use devenv's GHC installation.
        '';
      };
    };

    cabal = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = ''
          Whether to enable Cabal.
        '';
      };

      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.cabal-install;
        defaultText = lib.literalExpression "pkgs.cabal-install";
        description = ''
          Cabal package to use.
        '';
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      cfg.package
      zlib
      hpack
    ]
    ++ (lib.optional cfg.lsp.enable cfg.lsp.package)
    ++ (lib.optional cfg.cabal.enable cfg.cabal.package)
    ++ (lib.optional cfg.stack.enable stackWrapper);
  };
}
