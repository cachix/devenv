---
title: "openbao"
---

<!-- Do not edit this generated file. Edit docs/src/individual-docs instead. -->


[comment]: # (Please add your documentation above this line)

## Options

### services.openbao.enable



Whether to enable OpenBao server for secrets management.



*Type:*
boolean



*Default:*

```nix
false
```



*Example:*

```nix
true
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix)



### services.openbao.package



Which package of OpenBao to use.



*Type:*
package



*Default:*

```nix
pkgs.openbao
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix)



### services.openbao.clusterPort

The base port the OpenBao cluster listener listens on.
OpenBao always starts it; the actual port is allocated by devenv and may differ.



*Type:*
16 bit unsigned integer; between 0 and 65535 (both inclusive)



*Default:*

```nix
8201
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix)



### services.openbao.environmentVariables



Additional environment variables for the OpenBao server process.
Self-initialization requests can read them with the ` env ` source.



*Type:*
attribute set of string



*Default:*

```nix
{ }
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix)



### services.openbao.exportVaultEnv



Whether to also export ` VAULT_ADDR ` and ` VAULT_TOKEN `,
for clients that only support Vault, such as Terraform.



*Type:*
boolean



*Default:*

```nix
true
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix)



### services.openbao.host



The host address the OpenBao server listens on.
Wrap IPv6 addresses in brackets.



*Type:*
string



*Default:*

```nix
"127.0.0.1"
```



*Example:*

```nix
"0.0.0.0"
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix)



### services.openbao.initialize



Additional self-initialization blocks, run in order after the built-in ones
on the first start.
See [https://openbao.org/docs/configuration/self-init/](https://openbao.org/docs/configuration/self-init/).

Changes only apply after removing ` stateDir `.



*Type:*
list of (submodule)



*Default:*

```nix
[ ]
```



*Example:*

```nix
[
  {
    name = "userpass";
    requests = [
      {
        name = "enable-userpass";
        operation = "update";
        path = "sys/auth/userpass";
        data.type = "userpass";
      }
      {
        name = "add-admin";
        operation = "update";
        path = "auth/userpass/users/admin";
        data = {
          password = "admin";
          token_policies = [ "root" ];
        };
      }
    ];
  }
]

```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix)



### services.openbao.initialize.\*.name



The name of the block.



*Type:*
string matching the pattern \[A-Za-z_]\[A-Za-z0-9_-]\*

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix)



### services.openbao.initialize.\*.requests



The requests of the block, run in order.
Besides ` name `, ` operation ` and ` path `, requests accept ` data `, ` token `
and ` allow_failure `, as documented by OpenBao.



*Type:*
list of (open submodule of attribute set of (JSON value))

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix)



### services.openbao.initialize.\*.requests.\*.name



The name of the request, unique within its block.



*Type:*
string matching the pattern \[A-Za-z_]\[A-Za-z0-9_-]\*

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix)



### services.openbao.initialize.\*.requests.\*.operation



The operation of the request, such as ` read `, ` update ` or ` delete `.



*Type:*
string



*Example:*

```nix
"update"
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix)



### services.openbao.initialize.\*.requests.\*.path



The API path of the request, without the ` /v1/ ` prefix.



*Type:*
string



*Example:*

```nix
"sys/auth/userpass"
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix)



### services.openbao.kv.enable



Whether to mount a KV version 2 secrets engine during self-initialization.



*Type:*
boolean



*Default:*

```nix
true
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix)



### services.openbao.kv.path



The path of the KV secrets engine.



*Type:*
string



*Default:*

```nix
"secret"
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix)



### services.openbao.port



The base port the OpenBao API listens on.
The actual port is allocated by devenv and may differ;
` BAO_ADDR ` points to it.



*Type:*
16 bit unsigned integer; between 0 and 65535 (both inclusive)



*Default:*

```nix
8200
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix)



### services.openbao.settings



OpenBao server configuration, merged recursively over the generated configuration,
such as ` listener.tcp ` and ` storage.<backend> `.
See [https://openbao.org/docs/configuration/](https://openbao.org/docs/configuration/) for available settings.

Use ` initialize ` for self-initialization instead of ` settings.initialize `.



*Type:*
JSON value



*Default:*

```nix
{ }
```



*Example:*

```nix
{
  listener = {
    tcp = {
      max_request_duration = "30s";
    };
  };
  log_level = "debug";
}
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix)



### services.openbao.stateDir



The directory OpenBao stores its data and unseal key in.

Self-initialization only runs against empty storage: remove it to start over.



*Type:*
string



*Default:*

```nix
"${config.devenv.state}/openbao"
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix)



### services.openbao.storage.backend



The storage backend OpenBao keeps its data in, in a directory under ` stateDir `.

` pebbledb ` (OpenBao 2.7 and later) and ` file ` (before 2.7) start instantly.
` raft ` works with every version, but takes several seconds
to elect itself leader on the first start.



*Type:*
one of “pebbledb”, “raft”, “file”



*Default:*
` "pebbledb" ` for OpenBao 2.7 and later, ` "file" ` before

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix)



### services.openbao.token



The ID of a token with the root policy, created during self-initialization.
` BAO_TOKEN ` is set to it.

Changing it requires removing ` stateDir `, because self-initialization
only runs on the first start.



*Type:*
string



*Default:*

```nix
"root"
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix)



### services.openbao.ui.enable



Whether to serve the web UI at ` /ui `.



*Type:*
boolean



*Default:*

```nix
true
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix)



### services.openbao.unsealKeyFile



The 32-byte key the static seal uses to unseal OpenBao on startup.
A random key is generated if the file does not exist.



*Type:*
string



*Default:*

```nix
"${config.services.openbao.stateDir}/unseal.key"
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/openbao.nix)
