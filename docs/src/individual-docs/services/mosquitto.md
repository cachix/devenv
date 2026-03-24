Mosquitto provides a lightweight local MQTT broker for development and testing.

Enable it with:

```nix
{ ... }:

{
  services.mosquitto.enable = true;
}
```

By default, the broker listens on `127.0.0.1` and uses port `1883` through devenv's dynamic port allocation. The resolved port is exposed as `MOSQUITTO_PORT`, and the configured host is exposed as `MOSQUITTO_HOST`.

You can customize the listener or append native Mosquitto configuration:

```nix
{ ... }:

{
  services.mosquitto = {
    enable = true;
    bind = "127.0.0.1";
    port = 1883;
    extraConfig = ''
      max_queued_messages 1000
    '';
  };
}
```

`extraConfig` is appended to the generated `mosquitto.conf`.

[comment]: # (Please add your documentation on top of this line)

@AUTOGEN_OPTIONS@
