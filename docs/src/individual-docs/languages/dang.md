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

@AUTOGEN_OPTIONS@
