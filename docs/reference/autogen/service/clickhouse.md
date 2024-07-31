  # Clickhouse
  


## services\.clickhouse\.enable



Whether to enable clickhouse-server\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## services\.clickhouse\.package



Which package of clickhouse to use



*Type:*
package



*Default:*
` pkgs.clickhouse `



## services\.clickhouse\.config

ClickHouse configuration in YAML\.



*Type:*
strings concatenated with “\\n”



## services\.clickhouse\.httpPort



Which http port to run clickhouse on



*Type:*
signed integer



*Default:*
` 8123 `



## services\.clickhouse\.port



Which port to run clickhouse on



*Type:*
signed integer



*Default:*
` 9000 `
