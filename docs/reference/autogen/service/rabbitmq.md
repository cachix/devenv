  # Rabbitmq
  


## services\.rabbitmq\.enable



Whether to enable the RabbitMQ server, an Advanced Message
Queuing Protocol (AMQP) broker\.



*Type:*
boolean



*Default:*
` false `



## services\.rabbitmq\.package



Which rabbitmq package to use\.



*Type:*
package



*Default:*
` pkgs.rabbitmq-server `



## services\.rabbitmq\.configItems

Configuration options in RabbitMQ’s new config file format,
which is a simple key-value format that can not express nested
data structures\. This is known as the ` rabbitmq.conf ` file,
although outside NixOS that filename may have Erlang syntax, particularly
prior to RabbitMQ 3\.7\.0\.
If you do need to express nested data structures, you can use
` config ` option\. Configuration from ` config `
will be merged into these options by RabbitMQ at runtime to
form the final configuration\.
See [https://www\.rabbitmq\.com/configure\.html\#config-items](https://www\.rabbitmq\.com/configure\.html\#config-items)
For the distinct formats, see [https://www\.rabbitmq\.com/configure\.html\#config-file-formats](https://www\.rabbitmq\.com/configure\.html\#config-file-formats)



*Type:*
attribute set of string



*Default:*
` { } `



*Example:*

```
{
  "auth_backends.1.authn" = "rabbit_auth_backend_ldap";
  "auth_backends.1.authz" = "rabbit_auth_backend_internal";
}

```



## services\.rabbitmq\.cookie



Erlang cookie is a string of arbitrary length which must
be the same for several nodes to be allowed to communicate\.
Leave empty to generate automatically\.



*Type:*
string



*Default:*
` "" `



## services\.rabbitmq\.listenAddress



IP address on which RabbitMQ will listen for AMQP
connections\.  Set to the empty string to listen on all
interfaces\.  Note that RabbitMQ creates a user named
` guest ` with password
` guest ` by default, so you should delete
this user if you intend to allow external access\.
Together with ‘port’ setting it’s mostly an alias for
configItems\.“listeners\.tcp\.1” and it’s left for backwards
compatibility with previous version of this module\.



*Type:*
string



*Default:*
` "127.0.0.1" `



*Example:*
` "" `



## services\.rabbitmq\.managementPlugin\.enable



Whether to enable the management plugin\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## services\.rabbitmq\.managementPlugin\.port



On which port to run the management plugin



*Type:*
16 bit unsigned integer; between 0 and 65535 (both inclusive)



*Default:*
` 15672 `



## services\.rabbitmq\.nodeName



The name of the RabbitMQ node\.  This is used to identify
the node in a cluster\.  If you are running multiple
RabbitMQ nodes on the same machine, you must give each
node a unique name\.  The name must be of the form
` name@host `, where ` name ` is an arbitrary name and
` host ` is the domain name of the host\.



*Type:*
string



*Default:*
` "rabbit@localhost" `



## services\.rabbitmq\.pluginDirs



The list of directories containing external plugins



*Type:*
list of path



*Default:*
` [ ] `



## services\.rabbitmq\.plugins



The names of plugins to enable



*Type:*
list of string



*Default:*
` [ ] `



## services\.rabbitmq\.port



Port on which RabbitMQ will listen for AMQP connections\.



*Type:*
16 bit unsigned integer; between 0 and 65535 (both inclusive)



*Default:*
` 5672 `
