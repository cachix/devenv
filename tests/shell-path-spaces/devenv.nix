{
  # Simulate Python venv activation: an enterShell task re-prepends a directory
  # to PATH and exports PATH. Combined with a space-containing PATH entry this
  # used to glue `export PATH='…'` onto `eval "${shellHook:-}"` in shell-env.sh.
  tasks."test:venv-path" = {
    exec = ''
      mkdir -p "$DEVENV_STATE/venv/bin"
      export PATH="$DEVENV_STATE/venv/bin:$PATH"
      export VIRTUAL_ENV="$DEVENV_STATE/venv"
    '';
    exports = [
      "PATH"
      "VIRTUAL_ENV"
    ];
    before = [ "devenv:enterShell" ];
  };
}
