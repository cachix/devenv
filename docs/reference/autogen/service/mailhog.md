  # Mailhog
  


## services\.mailhog\.enable



Whether to enable mailhog process\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## services\.mailhog\.package



Which package of mailhog to use



*Type:*
package



*Default:*
` pkgs.mailhog `



## services\.mailhog\.additionalArgs

Additional arguments passed to ` mailhog `\.



*Type:*
list of strings concatenated with “\\n”



*Default:*
` [ ] `



*Example:*

```
[
  "-invite-jim"
]
```



## services\.mailhog\.apiListenAddress



Listen address for API\.



*Type:*
string



*Default:*
` "127.0.0.1:8025" `



## services\.mailhog\.smtpListenAddress



Listen address for SMTP\.



*Type:*
string



*Default:*
` "127.0.0.1:1025" `



## services\.mailhog\.uiListenAddress



Listen address for UI\.



*Type:*
string



*Default:*
` "127.0.0.1:8025" `
