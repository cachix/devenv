{ pkgs, config, lib, ... }:

let
  cfg = config.languages.elixir;

  beam-overlay = config.lib.getInput {
    name = "beam-overlay";
    url = "github:zeeetech/beam-overlay";
    attribute = "languages.elixir.version or languages.elixir.versionFile";
    follows = [ "nixpkgs" ];
  };

  # Apply the overlay to devenv's own pkgs instead of instantiating nixpkgs again.
  # Each version comes with the newest Erlang/OTP it supports, exposed as `.erlang`.
  elixirVersions = (beam-overlay.overlays.default pkgs pkgs).beam-overlay.elixir;

  # Read the Elixir version from an asdf/mise `.tool-versions` file (`elixir 1.17.3-otp-27`)
  # or from a file that holds only the version (`.elixir-version`).
  # The `-otp-NN` suffix is dropped: the overlay picks the newest OTP the version supports.
  versionFromFile = file:
    let
      content = builtins.readFile file;
      toolVersions = lib.filter (match: match != null)
        (map (builtins.match "[[:space:]]*elixir[[:space:]]+([^[:space:]#]+).*") (lib.splitString "\n" content));
      plain = builtins.match "[[:space:]]*([0-9][^[:space:]]*)[[:space:]]*" content;
      raw =
        if toolVersions != [ ] then builtins.head (builtins.head toolVersions)
        else if plain != null then builtins.head plain
        else throw "Could not find an Elixir version in ${toString file}. Expected a `.tool-versions` line like `elixir 1.17.3-otp-27`, or a file that holds only the version.";
    in
    builtins.head (lib.splitString "-otp-" raw);

  elixirVersion =
    if cfg.version != null then cfg.version
    else if cfg.versionFile != null then versionFromFile cfg.versionFile
    else null;

  projectDir =
    if lib.hasPrefix "/" cfg.directory
    then cfg.directory
    else "${config.devenv.root}/${cfg.directory}";

  mix = lib.getExe' cfg.package "mix";
  coreutils = pkgs.coreutils;
  mixDepsChecksumFile = "${config.devenv.state}/elixir/mix-deps.checksum";

  # Prints a checksum of everything that decides what `mix deps.get` fetches.
  # Runs from the project directory.
  mixDepsChecksum = pkgs.writeShellScript "elixir-mix-deps-checksum" ''
    for file in mix.exs mix.lock; do
      if [ -f "$file" ]; then
        ${coreutils}/bin/cksum < "$file" | ${coreutils}/bin/cut -d ' ' -f 1
      else
        echo "no-$file"
      fi
    done
    echo ${lib.escapeShellArg "${cfg.package}:${projectDir}"}
  '';
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

    versionFile = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = ''
        A file to read the Elixir version from.
        This can be an asdf or mise `.tool-versions` file with a line like `elixir 1.17.3-otp-27`,
        or a file that holds only the version, like `.elixir-version`.
        This automatically sets `languages.elixir.package` using [beam-overlay](https://github.com/zeeetech/beam-overlay).
        The `-otp-NN` suffix is ignored: devenv picks the newest Erlang/OTP that the Elixir version supports.

        ```nix
        languages.elixir.versionFile = ./.tool-versions;
        ```
      '';
      example = lib.literalExpression "./.tool-versions";
    };

    directory = lib.mkOption {
      type = lib.types.str;
      default = config.devenv.root;
      defaultText = lib.literalExpression "config.devenv.root";
      description = ''
        The Mix project's root directory, where `mix.exs` lives. Defaults to the root of the devenv project.
        Can be an absolute path or one relative to the root of the devenv project.
      '';
      example = "./directory";
    };

    mix.deps.enable = lib.mkEnableOption ''
      fetching Mix dependencies with `mix deps.get` when entering the shell.
      It only runs again when `mix.exs`, `mix.lock` or the Elixir package changes, or when the `deps` directory is missing.
      Hex and Rebar are installed into `MIX_HOME` first if they are missing
    '';

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
          if elixirVersion != null
          # Build the language server with the selected Elixir so it matches the project.
          then pkgs.beamPackages.elixir-ls.override { elixir = cfg.package; }
          else pkgs.beamPackages.elixir-ls;
        defaultText = lib.literalExpression ''
          if languages.elixir.version != null || languages.elixir.versionFile != null
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
          date = "2026-11-09";
          title = "languages.elixir keeps Mix's home directory inside the project";
          when = cfg.enable;
          description = ''
            `MIX_HOME` now defaults to `$DEVENV_STATE/mix`, so each project keeps its own Hex, Rebar, archives (like `phx_new`) and escripts.
            These are built for one Elixir and Erlang/OTP version, so sharing `~/.mix` between projects on different versions could break them.
            Run `mix local.hex` and reinstall any archives you need once per project, or set `env.MIX_HOME` to the full path of `~/.mix` to keep the old location.
            Escripts installed with `mix escript.install` are now on `PATH`.
          '';
        }
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
      assertions = [
        {
          assertion = cfg.version == null || cfg.versionFile == null;
          message = ''
            `languages.elixir.version` and `languages.elixir.versionFile` are both set.
            Only one of the two may be set. Remove one of the two options.
          '';
        }
      ];

      languages.elixir.package = lib.mkIf (elixirVersion != null) (
        elixirVersions.${elixirVersion}
          or (throw "Unsupported Elixir version '${elixirVersion}', see https://github.com/zeeetech/beam-overlay for supported versions")
      );

      # Hex, Rebar, archives and escripts are built for one Elixir/OTP version, so keep them per project.
      env.MIX_HOME = lib.mkDefault (config.env.DEVENV_STATE + "/mix");

      enterShell = ''
        export PATH="$MIX_HOME/escripts:$PATH"
      '';

      tasks."devenv:elixir:deps" = lib.mkIf cfg.mix.deps.enable {
        description = "Fetch Mix dependencies";
        cwd = projectDir;
        # Skip when nothing changed and the deps directory is still there.
        status = ''
          [ -f mix.exs ] && [ -d deps ] && [ -f ${lib.escapeShellArg mixDepsChecksumFile} ] \
            && [ "$(${mixDepsChecksum})" = "$(${coreutils}/bin/cat ${lib.escapeShellArg mixDepsChecksumFile})" ]
        '';
        exec = ''
          if [ ! -f mix.exs ]; then
            echo "No mix.exs found in ${projectDir}. Run 'mix new .' there to create a project." >&2
            exit 0
          fi

          ${coreutils}/bin/mkdir -p "$(${coreutils}/bin/dirname ${lib.escapeShellArg mixDepsChecksumFile})"
          {
            if ! ${lib.getExe pkgs.flock} -n 9; then
              echo "Waiting for another shell to finish fetching Mix dependencies..." >&2
              ${lib.getExe pkgs.flock} 9
            fi

            if ${mix} local.hex --force --if-missing \
              && ${mix} local.rebar --force --if-missing \
              && ${mix} deps.get; then
              ${mixDepsChecksum} > ${lib.escapeShellArg mixDepsChecksumFile}
            else
              echo "Fetching Mix dependencies failed. Run 'mix deps.get' manually." >&2
            fi
          } 9>${lib.escapeShellArg "${mixDepsChecksumFile}.lock"}
        '';
        before = [ "devenv:enterShell" ];
      };

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
