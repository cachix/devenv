{ pkgs, ... }:

{
  services.nats = {
    enable = true;

    # Enable HTTP monitoring (provides /healthz, /varz, /connz endpoints)
    monitoring.enable = true;

    # Enable JetStream for persistence and streaming
    jetstream = {
      enable = true;
      maxMemory = "1G";
      maxFileStore = "10G";
    };

    # Enable authorization
    authorization = {
      enable = true;
      user = "nats-user";
      password = "nats-pass";
    };
  };
}
