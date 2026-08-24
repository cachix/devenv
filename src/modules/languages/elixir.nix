{ pkgs, config, lib, ... }:

let
  cfg = config.languages.elixir;
in
{
  options.languages.elixir = {
    enable = lib.mkEnableOption "tools for Elixir development";

    package = lib.mkOption {
      type = lib.types.package;
      description = "Which Elixir package to use.";
      default = pkgs.beamPackages.elixir;
      defaultText = lib.literalExpression "pkgs.beamPackages.elixir";
    };

    lsp = {
      enable = lib.mkEnableOption "Elixir Language Server" // { default = true; };

      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.beamPackages.elixir-ls;
        defaultText = lib.literalExpression "pkgs.beamPackages.elixir-ls";
        description = "The Elixir language server package to use.";
      };
    };
  };

  config = lib.mkMerge [
    {
      changelogs = [
        {
          date = "2026-08-24";
          title = "languages.elixir.package and languages.elixir.lsp.package default to the pkgs.beamPackages set";
          when = cfg.enable;
          description = ''
            The default Elixir package is now `pkgs.beamPackages.elixir`, and the default Elixir language server is now `pkgs.beamPackages.elixir-ls`, following nixpkgs' deprecation of the top-level `elixir` attribute.
            This silences the "'elixir' is deprecated" evaluation warning on recent nixpkgs.
          '';
        }
      ];
    }
    (lib.mkIf cfg.enable {
      git-hooks.hooks = {
        credo.package = cfg.package;
        dialyzer.package = cfg.package;
        mix-format.package = cfg.package;
        mix-test.package = cfg.package;
      };

      packages = [
        cfg.package
      ] ++ lib.optional cfg.lsp.enable cfg.lsp.package;
    })
  ];
}
