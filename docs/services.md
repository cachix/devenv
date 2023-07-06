# Services

[processes](processes) are a low-level interface to starting a tool,
while services provide a higher level configuration.

Here's an example starting PostgreSQL with a few extensions:

```nix title="devenv.nix"
{ pkgs, ... }:

{
  services.postgres = {
    enable = true;
    package = pkgs.postgresql_15;
    initialDatabases = [{ name = "mydb"; }];
    extensions = extensions: [
      extensions.postgis
      extensions.timescaledb
    ];
    settings.shared_preload_libraries = "timescaledb";
    initialScript = "CREATE EXTENSION IF NOT EXISTS timescaledb;";
  };
}
```

Services start like processes with `devenv up`:

```shell-session

$ devenv up
Starting processes ...
```

## Supported services

{%
  include-markdown "services-all.md"
%}

You can find the definitions of all services [here](https://github.com/cachix/devenv/tree/main/src/modules/services).