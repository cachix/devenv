---
title: "tailwindcss"
---

<!-- Do not edit this generated file. Edit docs/src/individual-docs instead. -->


[comment]: # (Please add your documentation above this line)

## Options

### languages.tailwindcss.enable

Whether to enable tools for Tailwind CSS development.



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
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/tailwindcss.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/tailwindcss.nix)



### languages.tailwindcss.package



The Tailwind CSS package to use.



*Type:*
package



*Default:*

```nix
pkgs.tailwindcss_4
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/tailwindcss.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/tailwindcss.nix)



### languages.tailwindcss.lsp.enable



Whether to enable Tailwind CSS Language Server.



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
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/tailwindcss.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/tailwindcss.nix)



### languages.tailwindcss.lsp.package



The Tailwind CSS language server package to use.



*Type:*
package



*Default:*

```nix
pkgs.tailwindcss-language-server
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/tailwindcss.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/tailwindcss.nix)
