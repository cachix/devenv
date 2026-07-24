{ pkgs, config, ... }:
let
  # `services.sqld.port` is only the base of the allocation: the process manager
  # picks the next free port when the base is already taken on the machine.
  port = toString config.processes.sqld.ports.main.value;
in
{
  packages = with pkgs; [ turso-cli ];

  services.sqld = {
    enable = true;
    port = 6000;
  };

  scripts.sqld-check.exec = ''
    $DEVENV_PROFILE/bin/turso db shell http://127.0.0.1:${port} ".schema"
  '';

  enterTest = ''
    wait_for_port ${port}

    sqld-check
  '';
}
