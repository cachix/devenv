{ pkgs, config, lib, ... }:

let
  cfg = config.languages.clojure;
in
{
  options.languages.clojure = {
    enable = lib.mkEnableOption "tools for Clojure development";
    jdk.package = lib.mkOption {
      type = lib.types.package;
      example = lib.literalExpression "pkgs.jdk8";
      default = pkgs.jdk;
      defaultText = lib.literalExpression "pkgs.jdk";
      description = ''
        The JDK package to use.
        This will also become available as `JAVA_HOME`.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      (clojure.override {
        jdk = cfg.jdk.package;
      })
      clojure-lsp
    ];

    env.JAVA_HOME = cfg.jdk.package.home;
  };
}
