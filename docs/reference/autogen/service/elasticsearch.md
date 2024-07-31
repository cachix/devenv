  # Elasticsearch
  


## services\.elasticsearch\.enable



Whether to enable elasticsearch\.



*Type:*
boolean



*Default:*
` false `



## services\.elasticsearch\.package



Elasticsearch package to use\.



*Type:*
package



*Default:*
` pkgs.elasticsearch7 `



## services\.elasticsearch\.cluster_name

Elasticsearch name that identifies your cluster for auto-discovery\.



*Type:*
string



*Default:*
` "elasticsearch" `



## services\.elasticsearch\.extraCmdLineOptions



Extra command line options for the elasticsearch launcher\.



*Type:*
list of string



*Default:*
` [ ] `



## services\.elasticsearch\.extraConf



Extra configuration for elasticsearch\.



*Type:*
string



*Default:*
` "" `



*Example:*

```
''
  node.name: "elasticsearch"
  node.master: true
  node.data: false
''
```



## services\.elasticsearch\.extraJavaOptions



Extra command line options for Java\.



*Type:*
list of string



*Default:*
` [ ] `



*Example:*

```
[
  "-Djava.net.preferIPv4Stack=true"
]
```



## services\.elasticsearch\.listenAddress



Elasticsearch listen address\.



*Type:*
string



*Default:*
` "127.0.0.1" `



## services\.elasticsearch\.logging



Elasticsearch logging configuration\.



*Type:*
string



*Default:*

```
''
  logger.action.name = org.elasticsearch.action
  logger.action.level = info
  appender.console.type = Console
  appender.console.name = console
  appender.console.layout.type = PatternLayout
  appender.console.layout.pattern = [%d{ISO8601}][%-5p][%-25c{1.}] %marker%m%n
  rootLogger.level = info
  rootLogger.appenderRef.console.ref = console
''
```



## services\.elasticsearch\.plugins



Extra elasticsearch plugins



*Type:*
list of package



*Default:*
` [ ] `



*Example:*
` [ pkgs.elasticsearchPlugins.discovery-ec2 ] `



## services\.elasticsearch\.port



Elasticsearch port to listen for HTTP traffic\.



*Type:*
signed integer



*Default:*
` 9200 `



## services\.elasticsearch\.single_node



Start a single-node cluster



*Type:*
boolean



*Default:*
` true `



## services\.elasticsearch\.tcp_port



Elasticsearch port for the node to node communication\.



*Type:*
signed integer



*Default:*
` 9300 `
