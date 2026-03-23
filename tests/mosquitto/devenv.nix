{ config, pkgs, ... }:

let
  port = config.processes.mosquitto.ports.main.value;
in
{
  packages = [ pkgs.mosquitto ];

  services.mosquitto = {
    enable = true;
    port = 18830;
  };

  enterTest = ''
    wait_for_port ${toString port}

    message_file=$(mktemp)
    ${pkgs.mosquitto}/bin/mosquitto_sub \
      -h 127.0.0.1 \
      -p ${toString port} \
      -t devenv/test \
      -C 1 > "$message_file" &
    sub_pid=$!

    sleep 1

    ${pkgs.mosquitto}/bin/mosquitto_pub \
      -h 127.0.0.1 \
      -p ${toString port} \
      -t devenv/test \
      -m hello

    wait "$sub_pid"

    grep -qx "hello" "$message_file"
  '';
}
