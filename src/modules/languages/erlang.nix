{ pkgs, config, lib, ... }:

let
  cfg = config.languages.erlang;
  # rebar3 must be compiled with the selected Erlang so its BEAM files load on the user's OTP version.
  rebar3 = (pkgs.beam.packagesWith cfg.package).rebar3;
in
{
  options.languages.erlang = {
    enable = lib.mkEnableOption "tools for Erlang development";

    package = lib.mkOption {
      type = lib.types.package;
      description = "Which Erlang package to use.";
      default = pkgs.beamPackages.erlang;
      defaultText = lib.literalExpression "pkgs.beamPackages.erlang";
    };

    lsp = {
      enable = lib.mkEnableOption "Erlang Language Server" // { default = true; };

      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.erlang-language-platform;
        defaultText = lib.literalExpression "pkgs.erlang-language-platform";
        description = "The Erlang language server package to use.";
      };
    };
  };

  config = lib.mkMerge [
    {
      changelogs = [
        {
          date = "2026-07-22";
          title = "languages.erlang.package defaults to pkgs.beamPackages.erlang";
          when = cfg.enable;
          description = ''
            The default Erlang package is now `pkgs.beamPackages.erlang`, following nixpkgs' deprecation of the top-level `erlang` attribute.
            This silences the "'erlang' is deprecated" evaluation warning on recent nixpkgs.
            rebar3 is now built against the selected `languages.erlang.package`.
          '';
        }
      ];
    }
    (lib.mkIf cfg.enable {
      packages = [
        cfg.package
        rebar3
      ] ++ lib.optional cfg.lsp.enable cfg.lsp.package;
    })
  ];
}
