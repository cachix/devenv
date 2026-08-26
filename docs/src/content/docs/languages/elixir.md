---
title: "elixir"
---

<!-- Do not edit this generated file. Edit docs/src/individual-docs instead. -->



[comment]: # (Please add your documentation on top of this line)

## Options

### languages.elixir.enable

Whether to enable tools for Elixir development.



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
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/elixir.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/elixir.nix)



### languages.elixir.package



Which Elixir package to use.



*Type:*
package



*Default:*

```nix
pkgs.beamPackages.elixir
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/elixir.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/elixir.nix)



### languages.elixir.lsp.enable



Whether to enable Elixir Language Server.



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
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/elixir.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/elixir.nix)



### languages.elixir.lsp.package



The Elixir language server package to use.



*Type:*
package



*Default:*

```nix
pkgs.beamPackages.elixir-ls
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/elixir.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/elixir.nix)
