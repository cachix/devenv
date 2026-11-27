{ pkgs, config, lib, ... }:

let
  cfg = config.languages.deno;
  configurations = lib.optional cfg.enable cfg ++ lib.concatMap
    (name:
      let language = lib.attrByPath [ "languages" name ] { } config; in
      lib.optional ((language.enable or false) && language.deno.enable) language.deno
    ) [ "javascript" "typescript" ];
  package = (builtins.head configurations).package;
in
{
  options.languages.deno = {
    enable = lib.mkEnableOption "tools for Deno development";
    package = lib.mkOption {
      type = lib.types.package;
      description = "Which package of Deno to use.";
      default = pkgs.deno;
      defaultText = lib.literalExpression "pkgs.deno";
    };
  };

  config = lib.mkIf (configurations != [ ]) {
    packages = [
      package
    ];

    assertions = [
      {
        assertion = lib.all (configuration: toString configuration.package == toString package) configurations;
        message = "All enabled Deno integrations must use the same package; check languages.deno.package, languages.javascript.deno.package, and languages.typescript.deno.package.";
      }
    ];

    env.DENO_INSTALL_ROOT = config.env.DEVENV_STATE + "/deno";
    env.DENO_DIR = config.env.DENO_INSTALL_ROOT + "/cache";

    enterShell = ''
      export PATH="$PATH:$DENO_INSTALL_ROOT/bin"
    '';
  };
}
