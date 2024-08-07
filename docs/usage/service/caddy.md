  # Caddy
  


## services\.caddy\.enable



Whether to enable Caddy web server\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## services\.caddy\.package



Caddy package to use\.



*Type:*
package



*Default:*
` pkgs.caddy `



## services\.caddy\.adapter

Name of the config adapter to use\.
See [https://caddyserver\.com/docs/config-adapters](https://caddyserver\.com/docs/config-adapters) for the full list\.



*Type:*
string



*Default:*
` "caddyfile" `



*Example:*
` "nginx" `



## services\.caddy\.ca



Certificate authority ACME server\. The default (Let’s Encrypt
production server) should be fine for most people\. Set it to null if
you don’t want to include any authority (or if you want to write a more
fine-graned configuration manually)\.



*Type:*
null or string



*Default:*
` "https://acme-v02.api.letsencrypt.org/directory" `



*Example:*
` "https://acme-staging-v02.api.letsencrypt.org/directory" `



## services\.caddy\.config



Verbatim Caddyfile to use\.

Refer to [https://caddyserver\.com/docs/caddyfile](https://caddyserver\.com/docs/caddyfile)
for more information\.

Caddy v2 supports multiple config formats via adapters (see [` services.caddy.adapter `](\#servicescaddyconfig))\.



*Type:*
strings concatenated with “\\n”



*Default:*
` "" `



*Example:*

```
''
  # Global options block
  {
    debug
  }
  
  # Site block
  example.com {
    encode gzip
    log
    root /srv/http
  }
''
```



## services\.caddy\.dataDir



The data directory, for storing certificates\. Before 17\.09, this
would create a \.caddy directory\. With 17\.09 the contents of the
\.caddy directory are in the specified data directory instead\.
Caddy v2 replaced CADDYPATH with XDG directories\.
See [https://caddyserver\.com/docs/conventions\#file-locations](https://caddyserver\.com/docs/conventions\#file-locations)\.



*Type:*
path



*Default:*
` "/home/k3ys/sandbox/cachix/devenv/.devenv/state/caddy" `



## services\.caddy\.email



Email address (for Let’s Encrypt certificate)\.



*Type:*
string



*Default:*
` "" `



## services\.caddy\.resume



Use saved config, if any (and prefer over configuration passed with [` caddy.config `](\#caddyconfig))\.



*Type:*
boolean



*Default:*
` false `



## services\.caddy\.virtualHosts



Declarative vhost config\.



*Type:*
attribute set of (submodule)



*Default:*
` { } `



*Example:*

```
{
  "hydra.example.com" = {
    serverAliases = [ "www.hydra.example.com" ];
    extraConfig = ''''
      encode gzip
      log
      root /srv/http
    '''';
  };
};

```



## services\.caddy\.virtualHosts\.\<name>\.extraConfig



These lines go into the vhost verbatim\.



*Type:*
strings concatenated with “\\n”



*Default:*
` "" `



## services\.caddy\.virtualHosts\.\<name>\.serverAliases



Additional names of virtual hosts served by this virtual host configuration\.



*Type:*
list of string



*Default:*
` [ ] `



*Example:*

```
[
  "www.example.org"
  "example.org"
]
```
