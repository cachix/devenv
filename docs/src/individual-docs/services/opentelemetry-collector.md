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

@AUTOGEN_OPTIONS@
