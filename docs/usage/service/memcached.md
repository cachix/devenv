  # Memcached
  


## services\.memcached\.enable



Whether to enable memcached process\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## services\.memcached\.package



Which package of memcached to use



*Type:*
package



*Default:*
` pkgs.memcached `



## services\.memcached\.bind

The IP interface to bind to\.
` null ` means “all interfaces”\.



*Type:*
null or string



*Default:*
` "127.0.0.1" `



*Example:*
` "127.0.0.1" `



## services\.memcached\.port



The TCP port to accept connections\.
If port 0 is specified memcached will not listen on a TCP socket\.



*Type:*
16 bit unsigned integer; between 0 and 65535 (both inclusive)



*Default:*
` 11211 `



## services\.memcached\.startArgs



Additional arguments passed to ` memcached ` during startup\.



*Type:*
list of strings concatenated with “\\n”



*Default:*
` [ ] `



*Example:*

```
[
  "--memory-limit=100M"
]
```
