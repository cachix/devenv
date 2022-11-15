{ pkgs, config, lib, ... }:

let
  cfg = config.languages.java;
  inherit (lib) types mkEnableOption mkOption mkDefault mkIf optional;
in
{
  options.languages.java = {
    enable = mkEnableOption "Enable tools for Java development.";
    jdk.package = mkOption { type = types.package; };
    maven = {
      enable = mkEnableOption "maven";
      package = mkOption { type = types.package; };
    };
  };

  config = mkIf cfg.enable {
    languages.java.jdk.package = mkDefault pkgs.jdk;
    languages.java.maven.package = mkDefault (pkgs.maven.override { jdk = cfg.jdk.package; });
    packages = with pkgs; [
      gradle
    ] ++ (optional cfg.enable cfg.jdk.package)
    ++ (optional cfg.maven.enable cfg.maven.package);

    enterShell = ''
      mvn -version
    '';
    env.JAVA_HOME = cfg.jdk.package.home;
  };
}
