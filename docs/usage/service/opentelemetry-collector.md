  # Opentelemetry-collector
  


## services\.opentelemetry-collector\.enable



Whether to enable opentelemetry-collector\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## services\.opentelemetry-collector\.package



The OpenTelemetry Collector package to use



*Type:*
package



*Default:*
` pkgs.opentelemetry-collector-contrib `



## services\.opentelemetry-collector\.configFile

Override the configuration file used by OpenTelemetry Collector\.
By default, a configuration is generated from ` services.opentelemetry-collector.settings `\.

If overriding, enable the ` health_check ` extension to allow process-compose to check whether the Collector is ready\.
Otherwise, disable the readiness probe by setting ` processes.opentelemetry-collector.process-compose.readiness_probe = {}; `\.



*Type:*
null or path



*Default:*
` null `



*Example:*

```
pkgs.writeTextFile { name = "otel-config.yaml"; text = "..."; }

```



## services\.opentelemetry-collector\.settings



OpenTelemetry Collector configuration\.
Refer to https://opentelemetry\.io/docs/collector/configuration/
for more information on how to configure the Collector\.



*Type:*
YAML value



*Default:*

```
{
  extensions = {
    health_check = {
      endpoint = "localhost:13133";
    };
  };
  service = {
    extensions = [
      "health_check"
    ];
  };
}
```
