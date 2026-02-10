{ pkgs
, config
, lib
, ...
}:

let
  cfg = config.languages.zig;

  # Convert version like "0.15.1" to "0.15.0" for zls
  zlsVersion =
    if cfg.version != null
    then
      let
        versionParts = lib.splitString "." cfg.version;
        majorMinor = lib.init versionParts;
      in
      lib.concatStringsSep "." (majorMinor ++ [ "0" ])
    else null;

  zig-overlay = config.lib.getInput {
    name = "zig-overlay";
    url = "github:mitchellh/zig-overlay";
    attribute = "languages.zig.version";
    follows = [ "nixpkgs" ];
  };

  zls = config.lib.getInput {
    name = "zls";
    url = "github:zigtools/zls/${zlsVersion}";
    attribute = "languages.zig.version";
    follows = [ "nixpkgs" "zig-overlay" ];
  };
in
{
  imports = [
    (lib.mkRenamedOptionModule [ "languages" "zig" "zls" "package" ] [ "languages" "zig" "lsp" "package" ])
  ];

  options.languages.zig = {
    enable = lib.mkEnableOption "tools for Zig development";

    version = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = ''
        The Zig version to use.
        This automatically sets the `languages.zig.package` and `languages.zig.lsp.package` using [zig-overlay](https://github.com/mitchellh/zig-overlay).
      '';
      example = "0.15.1";
    };

    package = lib.mkOption {
      type = lib.types.package;
      description = "Which package of Zig to use.";
      default = pkgs.zig;
      defaultText = lib.literalExpression "pkgs.zig";
    };

    lsp = {
      enable = lib.mkEnableOption "Zig Language Server" // { default = true; };
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.zls;
        defaultText = lib.literalExpression "pkgs.zls";
        description = "The Zig language server package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    languages.zig.package = lib.mkIf (cfg.version != null) (
      zig-overlay.packages.${pkgs.stdenv.system}.${cfg.version}
    );

    languages.zig.lsp.package = lib.mkIf (cfg.version != null) (
      zls.packages.${pkgs.stdenv.system}.zls
    );

    packages = [
      cfg.package
    ] ++ lib.optional cfg.lsp.enable cfg.lsp.package;

    # The zig setup hook sets ZIG_GLOBAL_CACHE_DIR to a build sandbox path
    # via addEnvHooks, which overwrites env values. Restore env if set, else unset.
    enterShell =
      if config.env ? ZIG_GLOBAL_CACHE_DIR
      then ''export ZIG_GLOBAL_CACHE_DIR="${config.env.ZIG_GLOBAL_CACHE_DIR}"''
      else ''unset ZIG_GLOBAL_CACHE_DIR'';
  };
}
