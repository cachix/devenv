---
title: "opentelemetry-collector"
---

<!-- Do not edit this generated file. Edit docs/src/individual-docs instead. -->

## Automatic port allocation

Settings-generated configurations allocate free ports for the `health_check`
extension, OTLP gRPC/HTTP receivers, and internal Prometheus metrics readers.
Configured ports are allocation bases, not guaranteed listening ports. This lets
collectors in separate devenv projects run concurrently.

Use resolved ports when configuring applications or scrapers:

```nix
{ config, ... }:
{
  env.OTEL_EXPORTER_OTLP_ENDPOINT =
    "http://localhost:${toString config.processes.opentelemetry-collector.ports.http.value}";
}
```

The standard port names are `health` (base 13133), `metrics` (8888), `grpc`
(4317), and `http` (4318). Only configured, enabled listeners are allocated.
Named receivers such as `otlp/app` use `otlp-app-grpc` and `otlp-app-http`.
The first allocated internal Prometheus reader uses `metrics`; additional readers use
`metrics-<zero-based reader index>`. An omitted Prometheus reader port uses 8888.
Explicit reader lists replace the default reader; push exporters are unchanged.

Bind hosts and other collector settings are preserved. Disabled protocols,
disabled internal metrics, Unix sockets, port zero, and templated endpoints are
not automatically allocated. Other receivers, extensions, and listening
exporters remain manually configured.

### Raw configuration files

`configFile` overrides are passed through unchanged, without inferred port
allocations. When generating raw YAML in Nix, declare process ports and reference
their resolved values explicitly. Also override
`processes.opentelemetry-collector.ready` to match the raw file's health endpoint,
or set it to `lib.mkForce null` if health checks are disabled.

[comment]: # (Please add your documentation on top of this line)

## Options

### services.opentelemetry-collector.enable



Whether to enable opentelemetry-collector.



*Type:*
boolean



*Default:*

```nix
false
```



*Example:*

```nix
true
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/opentelemetry-collector.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/opentelemetry-collector.nix)



### services.opentelemetry-collector.package



The OpenTelemetry Collector package to use



*Type:*
package



*Default:*

```nix
pkgs.opentelemetry-collector-contrib
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/opentelemetry-collector.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/opentelemetry-collector.nix)



### services.opentelemetry-collector.configFile

Override the configuration file used by OpenTelemetry Collector.
By default, a configuration is generated from ` services.opentelemetry-collector.settings `.

Raw configuration files are not rewritten and their ports are not automatically allocated.
Declare process ports and reference their resolved values when generating a raw file,
and override the readiness probe to match its health endpoint.

If overriding, enable the ` health_check ` extension to allow the readiness probe to check whether the Collector is ready.
Otherwise, disable the readiness probe by setting ` processes.opentelemetry-collector.ready = lib.mkForce null; `.



*Type:*
null or absolute path



*Default:*

```nix
null
```



*Example:*

```nix
pkgs.writeTextFile { name = "otel-config.yaml"; text = "..."; }

```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/opentelemetry-collector.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/opentelemetry-collector.nix)



### services.opentelemetry-collector.settings



OpenTelemetry Collector configuration.
Literal TCP ports in the health_check extension, OTLP receivers, and internal
Prometheus metrics readers are used as allocation bases. The generated configuration
uses the resolved values from ` processes.opentelemetry-collector.ports `.
Refer to https://opentelemetry.io/docs/collector/configuration/
for more information on how to configure the Collector.



*Type:*
open submodule of (YAML 1.1 value)



*Default:*

```nix
defaultSettings
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/opentelemetry-collector.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/opentelemetry-collector.nix)
