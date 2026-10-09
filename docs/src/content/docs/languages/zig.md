---
title: "zig"
---

<!-- Do not edit this generated file. Edit docs/src/individual-docs instead. -->

## Getting Started

Use the `version` attribute to select the Zig compiler from [zig-overlay](https://github.com/mitchellh/zig-overlay)
and the matching [ZLS](https://github.com/zigtools/zls) (Zig Language Server) release:

```nix
languages.zig = {
  enable = true;
  version = "0.15.1";
};
```

This will automatically:

- Use the specified Zig version from zig-overlay
- Install the corresponding ZLS version (e.g., version "0.15.1" uses ZLS 0.15.0)

Add the required inputs before starting the environment. For the example above:

```sh
devenv inputs add zig-overlay github:mitchellh/zig-overlay --follows nixpkgs
devenv inputs add zls github:zigtools/zls/0.15.0 --follows nixpkgs --follows zig-overlay
```

For Zig 0.16.0, use `version = "0.16.0"` and add ZLS with
`devenv inputs add zls github:zigtools/zls/0.16.0 --follows nixpkgs`.
ZLS 0.16 uses its own `zig-flake` input instead of zig-overlay.
Setting `languages.zig.lsp.enable = false` disables ZLS and removes the need for the `zls` input.

Alternatively, you can manually specify packages:

```nix
languages.zig = {
  enable = true;
  package = pkgs.zig;
  lsp.package = pkgs.zls;
};
```

[comment]: # (Please add your documentation on top of this line)

## Options

### languages.zig.enable

Whether to enable tools for Zig development.



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
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/zig.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/zig.nix)



### languages.zig.package



Which package of Zig to use.



*Type:*
package



*Default:*

```nix
pkgs.zig
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/zig.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/zig.nix)



### languages.zig.lsp.enable



Whether to enable Zig Language Server.



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
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/zig.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/zig.nix)



### languages.zig.lsp.package



The Zig language server package to use.



*Type:*
package



*Default:*

```nix
pkgs.zls
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/zig.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/zig.nix)



### languages.zig.version



The Zig version to use.
This sets ` languages.zig.package ` using [zig-overlay](https://github.com/mitchellh/zig-overlay)
and ` languages.zig.lsp.package ` using the matching [ZLS](https://github.com/zigtools/zls) release.



*Type:*
null or string



*Default:*

```nix
null
```



*Example:*

```nix
"0.15.1"
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/zig.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/zig.nix)
