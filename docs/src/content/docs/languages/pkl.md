---
title: "pkl"
---

<!-- Do not edit this generated file. Edit docs/src/individual-docs instead. -->


[comment]: # (Please add your documentation above this line)

## Options

### languages.pkl.enable

Whether to enable tools for Pkl development.



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
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/pkl.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/pkl.nix)



### languages.pkl.package



The Pkl package to use.



*Type:*
package



*Default:*

```nix
pkgs.pkl
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/pkl.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/pkl.nix)



### languages.pkl.lsp.enable



Whether to enable Pkl Language Server.



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
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/pkl.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/pkl.nix)



### languages.pkl.lsp.package



The Pkl language server package to use.



*Type:*
package



*Default:*

```nix
pkgs.pkl-lsp
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/pkl.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/pkl.nix)
