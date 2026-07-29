---
title: "elm"
---

<!-- Do not edit this generated file. Edit docs/src/individual-docs instead. -->



[comment]: # (Please add your documentation on top of this line)

## Options

### languages.elm.enable

Whether to enable tools for Elm development.



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
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/elm.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/elm.nix)



### languages.elm.lsp.enable



Whether to enable Elm Language Server.



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
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/elm.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/elm.nix)



### languages.elm.lsp.package



The Elm language server package to use.



*Type:*
package



*Default:*

```nix
pkgs.elmPackages.elm-language-server
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/elm.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/elm.nix)
