{ pkgs
, config
, lib
, ...
}:
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

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable Scala development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable Metals language server (the standard LSP for Scala).";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.metals.override { jre = java.jdk.package; };
          defaultText = lib.literalExpression "pkgs.metals.override { jre = java.jdk.package; }";
          description = "The Metals package to use. Metals is the standard LSP implementation for Scala by Scalameta.";
        };
      };

      formatter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable scalafmt formatter.";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.scalafmt.override { jre = java.jdk.package; };
          defaultText = lib.literalExpression "pkgs.scalafmt.override { jre = java.jdk.package; }";
          description = "The scalafmt package to use.";
        };
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages =
      with pkgs;
      [
        (cfg.package.override { jre = java.jdk.package; })
        (coursier.override { jre = java.jdk.package; })
      ]
      ++ lib.optionals cfg.sbt.enable [
        (sbt.override (
          old:
          if (old ? "jre") then
            { jre = java.jdk.package; }
          else
            {
              sbt = old.sbt.override { jre = java.jdk.package; };
            }
        ))
      ]
      ++ lib.optionals cfg.mill.enable [
        (mill.override { jre = java.jdk.package; })
      ]
      ++ lib.optionals (lib.versionAtLeast java.jdk.package.version "17") [
        (scala-cli.override { jre = java.jdk.package; })
      ]
      ++ lib.optionals cfg.dev.enable (
        lib.optional cfg.dev.lsp.enable cfg.dev.lsp.package ++
        lib.optional cfg.dev.formatter.enable cfg.dev.formatter.package
      );

    languages.java.enable = true;
  };
}
