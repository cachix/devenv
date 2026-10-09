{ ... }:
{
  enterShell = ''
    echo DEVENV_SHELL_READY
  '';

  exitShell = ''
    touch "$DEVENV_ROOT/exit-shell-ran"
  '';

  tasks."test:before-exit" = {
    exec = ''touch "$DEVENV_ROOT/before-exit-ran"'';
    before = [ "devenv:exitShell" ];
  };
}
