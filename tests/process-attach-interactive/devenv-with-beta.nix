{ ... }:
{
  imports = [ ./devenv-alpha.nix ];
  processes.beta = {
    exec = ''touch "$DEVENV_STATE/beta-started"'';
    start.enable = false;
  };
}
