---
title: "sqld"
---

<!-- Do not edit this generated file. Edit docs/src/individual-docs instead. -->



[comment]: # (Please add your documentation on top of this line)

## Options

### services.sqld.enable

Whether to enable sqld.



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
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/sqld.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/sqld.nix)



### services.sqld.extraArgs



Add other sqld flags.



*Type:*
list of string



*Default:*

```nix
[ ]
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/sqld.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/sqld.nix)



### services.sqld.port



Port number to listen on.



*Type:*
16 bit unsigned integer; between 0 and 65535 (both inclusive)



*Default:*

```nix
8080
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/sqld.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/sqld.nix)
