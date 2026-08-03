---
title: "jsonnet"
---

<!-- Do not edit this generated file. Edit docs/src/individual-docs instead. -->



[comment]: # (Please add your documentation on top of this line)

## Options

### languages.jsonnet.enable

Whether to enable tools for jsonnet development.



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
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/jsonnet.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/jsonnet.nix)



### languages.jsonnet.lsp.enable



Whether to enable Jsonnet Language Server.



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
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/jsonnet.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/jsonnet.nix)



### languages.jsonnet.lsp.package



The Jsonnet language server package to use.



*Type:*
package



*Default:*

```nix
pkgs.jsonnet-language-server
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/jsonnet.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/jsonnet.nix)
