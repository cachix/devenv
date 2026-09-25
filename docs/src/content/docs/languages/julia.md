---
title: "julia"
---

<!-- Do not edit this generated file. Edit docs/src/individual-docs instead. -->

Enabling Julia also installs [Fatou](https://fatou.dev/), a language server,
formatter, and linter for Julia:

```nix
languages.julia.enable = true;
```

Configure your editor to run `fatou lsp` for Julia files. See Fatou's
[editor setup guide](https://fatou.dev/guide/editors.html) for details.

To disable the language server package:

```nix
languages.julia.lsp.enable = false;
```

Use `languages.julia.lsp.package` to select a different language server package.

[comment]: # (Please add your documentation on top of this line)

## Options

### languages.julia.enable

Whether to enable tools for Julia development.



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
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/julia.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/julia.nix)



### languages.julia.package



The Julia package to use.



*Type:*
package



*Default:*

```nix
pkgs.julia-bin
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/julia.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/julia.nix)



### languages.julia.lsp.enable



Whether to enable Julia Language Server.



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
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/julia.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/julia.nix)



### languages.julia.lsp.package



The Julia language server package to use.



*Type:*
package



*Default:*

```nix
pkgs.fatou
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/julia.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/julia.nix)
