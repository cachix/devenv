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
  options.languages.zig = {
    enable = lib.mkEnableOption "tools for Zig development";

    version = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = ''
        The Zig version to use.
        This automatically sets the `languages.zig.package` and `languages.zig.zls.package` using [zig-overlay](https://github.com/mitchellh/zig-overlay).
      '';
      example = "0.15.1";
    };

    package = lib.mkOption {
      type = lib.types.package;
      description = "Which package of Zig to use.";
      default = pkgs.zig;
      defaultText = lib.literalExpression "pkgs.zig";
    };

    zls.package = lib.mkOption {
      type = lib.types.package;
      description = "Which package of zls to use.";
      default = pkgs.zls;
      defaultText = lib.literalExpression "pkgs.zls";
    };
  };

  config = lib.mkIf cfg.enable {
    languages.zig.package = lib.mkIf (cfg.version != null) (
      zig-overlay.packages.${pkgs.stdenv.system}.${cfg.version}
    );

    languages.zig.zls.package = lib.mkIf (cfg.version != null) (
      zls.packages.${pkgs.stdenv.system}.zls
    );

    packages = [
      cfg.package
      cfg.zls.package
    ];
  };
}
