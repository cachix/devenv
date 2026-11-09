{ pkgs, config, lib, ... }:

let
  cfg = config.languages.elixir;

  beam-overlay = config.lib.getInput {
    name = "beam-overlay";
    url = "github:zeeetech/beam-overlay";
    attribute = "languages.elixir.version";
    follows = [ "nixpkgs" ];
  };

  # Apply the overlay to devenv's own pkgs instead of instantiating nixpkgs again.
  # Each version comes with the newest Erlang/OTP it supports, exposed as `.erlang`.
  elixirVersions = (beam-overlay.overlays.default pkgs pkgs).beam-overlay.elixir;
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

    version = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = ''
        The Elixir version to use.
        This automatically sets `languages.elixir.package` using [beam-overlay](https://github.com/zeeetech/beam-overlay),
        together with the newest Erlang/OTP runtime that the version supports.
        Elixir 1.15.0 and newer are available, including release candidates.
      '';
      example = "1.17.3";
    };

    erlang.package = lib.mkOption {
      type = lib.types.package;
      default = cfg.package.erlang or (lib.findFirst
        (package: (package.pname or "") == "erlang")
        (throw "Cannot determine the Erlang runtime for languages.elixir.package; set languages.elixir.erlang.package explicitly.")
        (cfg.package.buildInputs or [ ]));
      defaultText = lib.literalExpression "the Erlang runtime used to build languages.elixir.package";
      description = ''
        The Erlang runtime to include in the environment alongside Elixir.
        Defaults to the runtime used to build the selected Elixir package.
        Set this explicitly for custom packages that do not expose their Erlang dependency.
      '';
    };

    lsp = {
      enable = lib.mkEnableOption "Elixir Language Server" // { default = true; };

      package = lib.mkOption {
        type = lib.types.package;
        default =
          if cfg.version != null
          # Build the language server with the selected Elixir so it matches the project.
          then pkgs.beamPackages.elixir-ls.override { elixir = cfg.package; }
          else pkgs.beamPackages.elixir-ls;
        defaultText = lib.literalExpression ''
          if languages.elixir.version != null
          then pkgs.beamPackages.elixir-ls.override { elixir = languages.elixir.package; }
          else pkgs.beamPackages.elixir-ls
        '';
        description = "The Elixir language server package to use.";
      };
    };
  };

  config = lib.mkMerge [
    {
      changelogs = [
        {
          date = "2026-10-09";
          title = "languages.elixir now includes its matching Erlang runtime";
          when = cfg.enable;
          description = ''
            Elixir environments now provide `erl`, `erlc`, and `escript` from the Erlang runtime used to build the selected Elixir package.
            This prevents tools from using a missing or mismatched OTP runtime.
            Custom packages can select the runtime with `languages.elixir.erlang.package`.
          '';
        }
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
      languages.elixir.package = lib.mkIf (cfg.version != null) (
        elixirVersions.${cfg.version}
          or (throw "Unsupported Elixir version '${cfg.version}', see https://github.com/zeeetech/beam-overlay for supported versions")
      );

      git-hooks.hooks = {
        credo.package = cfg.package;
        dialyzer.package = cfg.package;
        mix-format.package = cfg.package;
        mix-test.package = cfg.package;
      };

      packages = [
        cfg.package
        cfg.erlang.package
      ] ++ lib.optional cfg.lsp.enable cfg.lsp.package;
    })
  ];
}
