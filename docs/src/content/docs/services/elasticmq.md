---
title: "elasticmq"
---

<!-- Do not edit this generated file. Edit docs/src/individual-docs instead. -->



[comment]: # (Please add your documentation on top of this line)

## Options

### services.elasticmq.enable

Whether to enable elasticmq-server.



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
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/elasticmq.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/elasticmq.nix)



### services.elasticmq.package



Which package of elasticmq-server-bin to use



*Type:*
package



*Default:*

```nix
pkgs.elasticmq-server-bin
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/elasticmq.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/elasticmq.nix)



### services.elasticmq.settings



Configuration for elasticmq-server



*Type:*
strings concatenated with “\\n”



*Default:*

```nix
""
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/elasticmq.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/elasticmq.nix)
