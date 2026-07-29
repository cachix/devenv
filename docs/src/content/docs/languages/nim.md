---
title: "nim"
---

<!-- Do not edit this generated file. Edit docs/src/individual-docs instead. -->



[comment]: # (Please add your documentation on top of this line)

## Options

### languages.nim.enable

Whether to enable tools for Nim development.



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
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/nim.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/nim.nix)



### languages.nim.package



The Nim package to use.



*Type:*
package



*Default:*

```nix
pkgs.nim
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/nim.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/nim.nix)



### languages.nim.lsp.enable



Whether to enable Nim Language Server.



*Type:*
boolean



*Default:*

```nix
true
```



*Example:*

```nix
true
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/nim.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/nim.nix)



### languages.nim.lsp.package



The Nim language server package to use.



*Type:*
package



*Default:*

```nix
pkgs.nimlangserver
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/nim.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/nim.nix)
