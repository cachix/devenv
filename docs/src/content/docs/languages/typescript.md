---
title: "typescript"
---

<!-- Do not edit this generated file. Edit docs/src/individual-docs instead. -->



[comment]: # (Please add your documentation on top of this line)

## Options

### languages.typescript.enable

Whether to enable tools for TypeScript development.



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
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/typescript.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/typescript.nix)



### languages.typescript.lsp.enable



Whether to enable TypeScript Language Server.



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
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/typescript.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/typescript.nix)



### languages.typescript.lsp.package



The TypeScript language server package to use.



*Type:*
package



*Default:*

```nix
pkgs.typescript-language-server
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/typescript.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/typescript.nix)
