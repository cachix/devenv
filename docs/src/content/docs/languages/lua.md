---
title: "lua"
---

<!-- Do not edit this generated file. Edit docs/src/individual-docs instead. -->



[comment]: # (Please add your documentation on top of this line)

## Options

### languages.lua.enable

Whether to enable tools for Lua development.



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
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/lua.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/lua.nix)



### languages.lua.package



The Lua package to use.



*Type:*
package



*Default:*

```nix
pkgs.lua
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/lua.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/lua.nix)



### languages.lua.lsp.enable



Whether to enable Lua Language Server.



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
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/lua.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/lua.nix)



### languages.lua.lsp.package



The Lua language server package to use.



*Type:*
package



*Default:*

```nix
pkgs.lua-language-server
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/lua.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/lua.nix)
