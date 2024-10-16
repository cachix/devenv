{ pkgs, config, lib, ... }:
let
  cfg = config.languages.scala;
  java = config.languages.java;
  sbt = cfg.sbt.package;
  mill = cfg.mill.package;
in
{
  options.languages.scala = {
    enable = lib.mkEnableOption "tools for Scala development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.scala_3;
      defaultText = lib.literalExpression "pkgs.scala_3";
      description = ''
        The Scala package to use.
      '';
    };

    sbt = with lib; {
      enable = mkEnableOption "sbt, the standard build tool for Scala";
      package = mkPackageOption pkgs "sbt" {
        default = "sbt";
        example = "sbt-with-scala-native";
      };
    };

    mill = with lib; {
      enable = mkEnableOption "mill, a simplified, fast build tool for Scala";
      package = mkPackageOption pkgs "mill" {
        default = "mill";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      (cfg.package.override
        { jre = java.jdk.package; })
      (metals.override
        { jre = java.jdk.package; })
      (coursier.override
        { jre = java.jdk.package; })
      (scalafmt.override
        { jre = java.jdk.package; })
    ] ++ lib.optionals cfg.sbt.enable [
      (sbt.override
        { jre = java.jdk.package; })
    ] ++ lib.optionals cfg.mill.enable [
      (mill.override
        { jre = java.jdk.package; })
    ] ++ lib.optionals (lib.versionAtLeast java.jdk.package.version "17") [
      (scala-cli.override
        { jre = java.jdk.package; })
    ];

    languages.java.enable = true;
  };
}
