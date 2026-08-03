---
title: "vault"
---

<!-- Do not edit this generated file. Edit docs/src/individual-docs instead. -->



[comment]: # (Please add your documentation on top of this line)

## Options

### services.vault.enable



Whether to enable vault process.



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
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/vault.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/vault.nix)



### services.vault.package



Which package of Vault to use.



*Type:*
package



*Default:*

```nix
pkgs.vault-bin
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/vault.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/vault.nix)



### services.vault.address

Specifies the address to bind to for listening



*Type:*
string



*Default:*

```nix
"127.0.0.1:8200"
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/vault.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/vault.nix)



### services.vault.disableClustering



Specifies whether clustering features such as request forwarding are enabled



*Type:*
boolean



*Default:*

```nix
true
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/vault.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/vault.nix)



### services.vault.disableMlock



Disables the server from executing the mlock syscall



*Type:*
boolean



*Default:*

```nix
true
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/vault.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/vault.nix)



### services.vault.ui



Enables the built-in web UI



*Type:*
boolean



*Default:*

```nix
true
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/vault.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/vault.nix)
