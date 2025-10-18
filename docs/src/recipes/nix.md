## Nix patterns

### Getting a recent version of a package from `nixpkgs-unstable`

By default, new devenv projects are configured to use a fork of `nixpkgs` called [`devenv-nixpkgs/rolling`](https://github.com/cachix/devenv-nixpkgs).

`devenv-nixpkgs/rolling` is tested against devenv's test suite and receives monthly updates, as well as interim stability patches that affect devenv's most popular services and integrations.

For some packages that are updated frequently, you may want to use a more recent version from `nixpkgs-unstable`.

1. Add `nixpkgs-unstable` input to `devenv.yaml`:

   ```yaml title="devenv.yaml" hl_lines="4-5"
   inputs:
     nixpkgs:
       url: github:cachix/devenv-nixpkgs/rolling
     nixpkgs-unstable:
       url: github:NixOS/nixpkgs/nixpkgs-unstable
   ```

2. Use the package in your `devenv.nix`:

   ```nix title="devenv.nix" hl_lines="3 7"
   { pkgs, inputs, ... }:
   let
     pkgs-unstable = import inputs.nixpkgs-unstable { system = pkgs.stdenv.system; };
   in
   {
     packages = [
       pkgs-unstable.elmPackages.elm-test-rs
     ];
   }

   ```

### How do I contribute a package or a fix to `nixpkgs`?

For temporary fixes, we recommend using either [overlays](/overlays.md) or a [different nixpkgs input](#getting-a-recent-version-of-a-package-from-nixpkgs-unstable).

You can also consider contributing your changes back to [`nixpkgs`](https://github.com/NixOS/nixpkgs).
Follow the [nixpkgs contributing guide](https://github.com/NixOS/nixpkgs/blob/master/pkgs/README.md) to get started.

Once you've forked and cloned `nixpkgs`, test your changes with devenv:

```yaml
inputs:
  nixpkgs:
    url: github:username/nixpkgs/branch
    # Or a local path to nixpkgs
    # url: path:/path/to/local/nixpkgs/clone
```

### Add a directory to `$PATH`

This example adds Elixir install scripts to `~/.mix/escripts`:

```nix title="devenv.nix"
{ ... }:

{
  languages.elixir.enable = true;

  enterShell = ''
    export PATH="$HOME/.mix/escripts:$PATH"
  '';
}
```

### Escape Nix curly braces inside shell scripts

```nix title="devenv.nix"
{ pkgs, ... }: {
  scripts.myscript.exec = ''
    foobar=1
    echo ''${foobar}
  '';
}
```

