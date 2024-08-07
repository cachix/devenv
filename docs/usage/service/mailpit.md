  # Mailpit
  


## services\.mailpit\.enable



Whether to enable mailpit process\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## services\.mailpit\.package



Which package of mailpit to use



*Type:*
package



*Default:*
` pkgs.mailpit `



## services\.mailpit\.additionalArgs

Additional arguments passed to ` mailpit `\.



*Type:*
list of strings concatenated with “\\n”



*Default:*
` [ ] `



*Example:*

```
[
  "--max=500"
]
```



## services\.mailpit\.smtpListenAddress



Listen address for SMTP\.



*Type:*
string



*Default:*
` "127.0.0.1:1025" `



## services\.mailpit\.uiListenAddress



Listen address for UI\.



*Type:*
string



*Default:*
` "127.0.0.1:8025" `
