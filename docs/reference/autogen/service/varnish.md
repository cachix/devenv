  # Varnish
  


## services\.varnish\.enable

Whether to enable Varnish process and expose utilities\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## services\.varnish\.package



Which Varnish package to use\.



*Type:*
package



*Default:*
` pkgs.varnish `



## services\.varnish\.extraModules



Varnish modules (except ‘std’)\.



*Type:*
list of package



*Default:*
` [ ] `



*Example:*
` [ pkgs.varnish73Packages.modules ] `



## services\.varnish\.listen



Which address to listen on\.



*Type:*
string



*Default:*
` "127.0.0.1:6081" `



## services\.varnish\.memorySize



How much memory to allocate to Varnish\.



*Type:*
string



*Default:*
` "64M" `



## services\.varnish\.vcl



Varnish VCL configuration\.



*Type:*
strings concatenated with “\\n”



*Default:*

```
''
  vcl 4.0;
  
  backend default {
    .host = "127.0.0.1";
    .port = "80";
  }
''
```
