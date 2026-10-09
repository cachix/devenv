---
title: "elixir"
---

<!-- Do not edit this generated file. Edit docs/src/individual-docs instead. -->

Enable Elixir with:

```nix
languages.elixir.enable = true;
```

Since devenv 2.4.1, this also provides `erl`, `erlc`, and `escript` from the Erlang runtime used to build your selected Elixir package. Selecting a different `languages.elixir.package` automatically selects its matching runtime.

For custom packages that do not expose their Erlang dependency, set `languages.elixir.erlang.package` explicitly to the runtime used to build that package.


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



### languages.elixir.erlang.package



The Erlang runtime to include in the environment alongside Elixir.
Defaults to the runtime used to build the selected Elixir package.
Set this explicitly for custom packages that do not expose their Erlang dependency.



*Type:*
package



*Default:*

```nix
the Erlang runtime used to build languages.elixir.package
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
