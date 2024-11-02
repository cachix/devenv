{ pkgs, config, lib, ... }:

let
  cfg = config.languages.java;
  mavenArgs = lib.functionArgs pkgs.maven.override;
  mavenPackage =
    if builtins.hasAttr "jdk" mavenArgs then
    # ensure backwards compatibility when using pkgs from before this commit: https://github.com/NixOS/nixpkgs/commit/ea0bc3224593ddf7ac6c702c7acb6c89cf188f0f
      pkgs.maven.override { jdk = cfg.jdk.package; }
    else
      pkgs.maven.override { jdk_headless = cfg.jdk.package; };
  inherit (lib) types mkEnableOption mkOption mkDefault mkIf optional literalExpression;
in
{
  options.languages.java = {
    enable = mkEnableOption "tools for Java development";
    jdk.package = mkOption {
      type = types.package;
      example = literalExpression "pkgs.jdk8";
      default = pkgs.jdk;
      defaultText = literalExpression "pkgs.jdk";
      description = ''
        The JDK package to use.
        This will also become available as `JAVA_HOME`.
      '';
    };
    maven = {
      enable = mkEnableOption "maven";
      package = mkOption {
        type = types.package;
        defaultText = literalExpression "pkgs.maven.override { jdk_headless = cfg.jdk.package; }";
        description = ''
          The Maven package to use.
          The Maven package by default inherits the JDK from `languages.java.jdk.package`.
        '';
      };
    };
    gradle = {
      enable = mkEnableOption "gradle";
      package = mkOption {
        type = types.package;
        defaultText = literalExpression "pkgs.gradle.override { java = cfg.jdk.package; }";
        description = ''
          The Gradle package to use.
          The Gradle package by default inherits the JDK from `languages.java.jdk.package`.
        '';
      };
    };
  };

  config = mkIf cfg.enable {
    languages.java.maven.package = mkDefault mavenPackage;
    languages.java.gradle.package = mkDefault (pkgs.gradle.override { java = cfg.jdk.package; });
    packages = (optional cfg.enable cfg.jdk.package)
      ++ (optional cfg.maven.enable cfg.maven.package)
      ++ (optional cfg.gradle.enable cfg.gradle.package);

    env.JAVA_HOME = cfg.jdk.package.home;
  };
}
