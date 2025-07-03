{ pkgs, config, lib, ... }:

let
  cfg = config.languages.deno;
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

    dev = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Enable Deno development tools.";
      };

      lsp = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable deno language server.";
        };
      };

      formatter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable deno formatter.";
        };
      };

      linter = {
        enable = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Enable deno linter.";
        };
      };
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      cfg.package
    ];

    env.DENO_INSTALL_ROOT = config.env.DEVENV_STATE + "/deno";
    env.DENO_DIR = config.env.DENO_INSTALL_ROOT + "/cache";

    enterShell = ''
      export PATH="$PATH:$DENO_INSTALL_ROOT/bin"
    '';
  };
}
