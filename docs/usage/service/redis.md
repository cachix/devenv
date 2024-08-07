  # Redis
  


## services\.redis\.enable



Whether to enable Redis process and expose utilities\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## services\.redis\.package



Which package of Redis to use



*Type:*
package



*Default:*
` pkgs.redis `



## services\.redis\.bind

The IP interface to bind to\.
` null ` means “all interfaces”\.



*Type:*
null or string



*Default:*
` "127.0.0.1" `



*Example:*
` "127.0.0.1" `



## services\.redis\.extraConfig



Additional text to be appended to ` redis.conf `\.



*Type:*
strings concatenated with “\\n”



*Default:*
` "locale-collate C" `



## services\.redis\.port



The TCP port to accept connections\.
If port 0 is specified Redis, will not listen on a TCP socket\.



*Type:*
16 bit unsigned integer; between 0 and 65535 (both inclusive)



*Default:*
` 6379 `
