{ pkgs, config, ... }:
{
  languages.elixir = {
    enable = true;
    lsp.enable = false;
    directory = "./app";
    mix.deps.enable = true;
  };

  enterTest = ''
    # Mix keeps Hex, Rebar and archives inside the project.
    test "$MIX_HOME" = "${config.env.DEVENV_STATE}/mix"
    ls "$MIX_HOME"/archives/hex-*

    # Entering the shell fetched the dependencies of the project in ./app.
    test -d app/deps/jason
    test -f app/mix.lock

    # The task rewrites its checksum file each time it runs.
    checksum_file="${config.devenv.state}/elixir/mix-deps.checksum"
    modified() { ${pkgs.coreutils}/bin/stat -c %y "$checksum_file"; }
    before=$(modified)

    # Nothing changed, so the task is skipped.
    devenv tasks run devenv:elixir:deps
    test "$(modified)" = "$before"

    # A missing deps directory makes it run again.
    rm -rf app/deps
    devenv tasks run devenv:elixir:deps
    test -d app/deps/jason
    test "$(modified)" != "$before"

    # A missing Hex makes it run again and reinstall Hex.
    rm -rf "$MIX_HOME"/archives/hex-*
    devenv tasks run devenv:elixir:deps
    ls "$MIX_HOME"/archives/hex-*

    # Escripts installed with Mix are on PATH.
    case ":$PATH:" in
      *":$MIX_HOME/escripts:"*) ;;
      *) echo "$MIX_HOME/escripts is not on PATH" >&2; exit 1 ;;
    esac
  '';
}
