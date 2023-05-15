{ pkgs, config, lib, ... }:

let
  cfg = config.languages.deno;
in
{
  options.languages.deno = {
    enable = lib.mkEnableOption "tools for Deno development";
  };

  config = lib.mkIf cfg.enable {
    packages = [
      pkgs.deno
    ];

    env.DENO_INSTALL_ROOT = config.env.DEVENV_STATE + "/deno";
    env.DENO_DIR = config.env.DENO_INSTALL_ROOT + "/cache";

    enterShell = ''
      export PATH="$PATH:$DENO_INSTALL_ROOT/bin"
    '';
  };
}
