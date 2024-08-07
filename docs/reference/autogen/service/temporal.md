  # Temporal
  


## services\.temporal\.enable

Whether to enable Temporal process\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## services\.temporal\.package



Which package of Temporal to use\.



*Type:*
package



*Default:*
` pkgs.temporal-cli `



## services\.temporal\.ip



IPv4 address to bind the frontend service to\.



*Type:*
string



*Default:*
` "127.0.0.1" `



## services\.temporal\.namespaces



Specify namespaces that should be pre-created (namespace “default” is always created)\.



*Type:*
list of string



*Default:*
` [ ] `



*Example:*

```
[
  "my-namespace"
  "my-other-namespace"
]
```



## services\.temporal\.port



Port for the frontend gRPC service\.



*Type:*
16 bit unsigned integer; between 0 and 65535 (both inclusive)



*Default:*
` 7233 `



## services\.temporal\.state



State configuration\.



*Type:*
submodule



*Default:*
` { } `



## services\.temporal\.state\.ephemeral



When enabled, the Temporal state gets lost when the process exists\.



*Type:*
boolean



*Default:*
` true `



## services\.temporal\.state\.sqlite-pragma



Sqlite pragma statements



*Type:*
attribute set of string



*Default:*
` { } `



*Example:*

```
{
  journal_mode = "wal";
  synchronous = "2";
}
```



## services\.temporal\.ui



UI configuration\.



*Type:*
submodule



*Default:*
` { } `



## services\.temporal\.ui\.enable



Enable the Web UI\.



*Type:*
boolean



*Default:*
` true `



## services\.temporal\.ui\.ip



IPv4 address to bind the Web UI to\.



*Type:*
string



*Default:*
` "127.0.0.1" `



## services\.temporal\.ui\.port



Port for the Web UI\.



*Type:*
16 bit unsigned integer; between 0 and 65535 (both inclusive)



*Default:*
[` services.temporal.port `](\#servicestemporalport) + 1000
