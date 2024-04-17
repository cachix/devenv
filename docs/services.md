# Services

[processes](processes.md) are a low-level interface to starting a tool,
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

Service states are persisted to directories in `$DEVENV_STATE`. When you adjust options like the above used `initialScript`, you will have to delete the service's directory for changes to take effect on next `devenv up`.

## Services in the background

Services start in the foreground by default. If you want to start services up in the background, you can pass the `-d` flag:

```shell-session

$ devenv up -d
```

## Supported services

{%
  include-markdown "services-all.md"
%}

You can find all supported options for services [here](https://devenv.sh/reference/options/#servicesadminerenable).
