---
title: "dang"
---

<!-- Do not edit this generated file. Edit docs/src/individual-docs instead. -->

[Dang](https://github.com/vito/dang) is an experimental GraphQL scripting language, used for writing [Dagger](https://dagger.io/) modules.

Added in devenv 2.4.1.

```nix title="devenv.nix"
{
  languages.dang.enable = true;
}
```

`languages.dang.package` defaults to `pkgs.dang` when your nixpkgs (or an overlay) provides it, and otherwise to a package bundled with devenv.

To get Dang together with the Dagger CLI, use the [Dagger integration](/integrations/dagger/) with `dagger.dang.enable = true`.

[comment]: # (Please add your documentation on top of this line)

## Options

### languages.dang.enable

Whether to enable tools for Dang development.



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
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/dang](https://github.com/cachix/devenv/blob/main/src/modules/languages/dang)



### languages.dang.package



The Dang package to use.

Defaults to ` pkgs.dang ` when available, otherwise to a package bundled with devenv.



*Type:*
package



*Default:*

```nix
pkgs.dang or (pkgs.callPackage ./package.nix { })
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/dang](https://github.com/cachix/devenv/blob/main/src/modules/languages/dang)
