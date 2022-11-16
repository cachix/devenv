{ pkgs, config, lib, ... }:

let
  cfg = config.languages.java;
  inherit (lib) types mkEnableOption mkOption mkDefault mkIf mdDoc optional literalExpression;
in
{
  options.languages.java = {
    enable = mkEnableOption "tools for Java development";
    jdk.package = mkOption {
      type = types.package;
      example = pkgs.jdk8;
      default = pkgs.jdk;
      defaultText = literalExpression "pkgs.jdk";
      description = mdDoc ''
        The JDK package to use.
        This will also become available as `JAVA_HOME`.
      '';
    };
    maven = {
      enable = mkEnableOption "maven";
      package = mkOption {
        type = types.package;
        defaultText = "pkgs.maven.override { jdk = cfg.jdk.package; }";
        description = mdDoc ''
          The maven package to use.
          The maven package by default inherits the JDK from `languages.java.jdk.package`.
        '';
      };
    };
    gradle = {
      enable = mkEnableOption "gradle";
      package = mkOption {
        type = types.package;
        default = pkgs.gradle;
        defaultText = literalExpression "pkgs.gradle";
        description = ''
          The gradle package to use.
        '';
      };
    };
  };

  config = mkIf cfg.enable {
    languages.java.maven.package = mkDefault (pkgs.maven.override { jdk = cfg.jdk.package; });
    packages = (optional cfg.enable cfg.jdk.package)
      ++ (optional cfg.maven.enable cfg.maven.package)
      ++ (optional cfg.gradle.enable cfg.gradle.package);

    env.JAVA_HOME = cfg.jdk.package.home;
  };
}
