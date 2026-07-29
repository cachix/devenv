---
title: "opentofu"
---

<!-- Do not edit this generated file. Edit docs/src/individual-docs instead. -->



[comment]: # (Please add your documentation on top of this line)

## Options

### languages.opentofu.enable

Whether to enable tools for OpenTofu development.



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
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/opentofu.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/opentofu.nix)



### languages.opentofu.package



The OpenTofu package to use.



*Type:*
package



*Default:*

```nix
pkgs.opentofu
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/opentofu.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/opentofu.nix)



### languages.opentofu.lsp.enable



Whether to enable OpenTofu Language Server.



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
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/opentofu.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/opentofu.nix)



### languages.opentofu.lsp.package



The OpenTofu language server package to use.



*Type:*
package



*Default:*

```nix
pkgs.terraform-ls
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/opentofu.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/opentofu.nix)
