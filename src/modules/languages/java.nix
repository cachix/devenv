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
  inherit (lib) types mkEnableOption mkOption mkDefault mkIf optional optionals literalExpression;
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

    dev = {
      enable = mkOption {
        type = types.bool;
        default = true;
        description = "Enable Java development tools.";
      };

      lsp = {
        enable = mkOption {
          type = types.bool;
          default = true;
          description = "Enable jdt-language-server language server.";
        };
        package = mkOption {
          type = types.package;
          default = pkgs.jdt-language-server;
          defaultText = literalExpression "pkgs.jdt-language-server";
          description = ''
            The jdt-language-server package to use.
            
            By default, this uses Eclipse JDT Language Server, which is the most popular
            and feature-rich Java LSP implementation.
            
            Alternative LSP servers available in nixpkgs:
            - vscode-extensions.redhat.java (Red Hat's Java extension which includes JDT LS)
            - metals (for Scala/Java projects)
            
            Note: vscode-javac and Apache NetBeans Java LSP are not currently packaged in nixpkgs.
          '';
        };
      };

      formatter = {
        enable = mkOption {
          type = types.bool;
          default = true;
          description = "Enable google-java-format formatter.";
        };
        package = mkOption {
          type = types.package;
          default = pkgs.google-java-format;
          defaultText = literalExpression "pkgs.google-java-format";
          description = "The google-java-format package to use.";
        };
      };

      debugger = {
        enable = mkOption {
          type = types.bool;
          default = true;
          description = "Enable java-debug debugger.";
        };
        package = mkOption {
          type = types.package;
          default = pkgs.java-debug or null;
          defaultText = literalExpression "pkgs.java-debug";
          description = "The java-debug package to use.";
        };
      };
    };
  };

  config = mkIf cfg.enable {
    languages.java.maven.package = mkDefault mavenPackage;
    languages.java.gradle.package = mkDefault (pkgs.gradle.override { java = cfg.jdk.package; });
    packages = [
      cfg.jdk.package
    ] ++ optional cfg.maven.enable cfg.maven.package
    ++ optional cfg.gradle.enable cfg.gradle.package
    ++ optionals cfg.dev.enable (
      optional cfg.dev.lsp.enable cfg.dev.lsp.package ++
        optional cfg.dev.formatter.enable cfg.dev.formatter.package ++
        optional (cfg.dev.debugger.enable && cfg.dev.debugger.package != null) cfg.dev.debugger.package
    );

    env.JAVA_HOME = cfg.jdk.package.home;
  };
}
