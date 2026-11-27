{ ... }:
{
  process.manager.implementation = "native";
  services.rabbitmq.enable = true;
  processes.rabbitmq.ports.epmd.allocate = 34369;

  processes.record-rabbitmq-port = {
    after = [ "devenv:processes:rabbitmq@ready" ];
    exec = ''
      printf '%s\n' "$ERL_EPMD_PORT" > "$DEVENV_STATE/epmd-port"
    '';
  };
}
