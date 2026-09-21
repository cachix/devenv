{ pkgs, config, lib, ... }:

let
  cfg = config.languages.julia;
in
{
  options.languages.julia = {
    enable = lib.mkEnableOption "tools for Julia development";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.julia-bin;
      defaultText = lib.literalExpression "pkgs.julia-bin";
      description = "The Julia package to use.";
    };

    lsp = {
      enable = lib.mkEnableOption "Julia Language Server" // { default = true; };

      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.fatou;
        defaultText = lib.literalExpression "pkgs.fatou";
        description = "The Julia language server package to use.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    changelogs = [
      {
        date = "2026-09-21";
        title = "Julia includes the Fatou language server by default";
        when = cfg.lsp.enable;
        description = ''
          Enabling `languages.julia.enable` now installs Fatou, a language server, formatter, and linter for Julia.
          Configure your editor to run `fatou lsp` for Julia files.
          Set `languages.julia.lsp.enable = false` to disable it, or use `languages.julia.lsp.package` to select another package.
        '';
      }
    ];

    packages = [
      cfg.package
    ] ++ lib.optional cfg.lsp.enable cfg.lsp.package;
  };
}
