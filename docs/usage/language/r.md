  # R
  


## devcontainer\.enable

Whether to enable generation \.devcontainer\.json for devenv integration\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## devcontainer\.settings



Devcontainer settings\.



*Type:*
JSON value



*Default:*
` { } `



## devcontainer\.settings\.customizations\.vscode\.extensions



List of preinstalled VSCode extensions\.



*Type:*
list of string



*Default:*

```
[
  "mkhl.direnv"
]
```



## devcontainer\.settings\.image



The name of an image in a container registry\.



*Type:*
string



*Default:*
` "ghcr.io/cachix/devenv:latest" `



## devcontainer\.settings\.overrideCommand



Override the default command\.



*Type:*
anything



*Default:*
` false `



## devcontainer\.settings\.updateContentCommand



Command to run after container creation\.



*Type:*
anything



*Default:*
` "devenv test" `



## languages\.elixir\.enable



Whether to enable tools for Elixir development\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## languages\.elixir\.package



Which package of Elixir to use\.



*Type:*
package



*Default:*
` pkgs.elixir `



## languages\.r\.enable



Whether to enable tools for R development\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## languages\.r\.package



The R package to use\.



*Type:*
package



*Default:*
` pkgs.R `



## services\.adminer\.enable



Whether to enable Adminer process\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## services\.adminer\.package



Which package of Adminer to use\.



*Type:*
package



*Default:*
` pkgs.adminer `



## services\.adminer\.listen



Listen address for the Adminer\.



*Type:*
string



*Default:*
` "127.0.0.1:8080" `



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
