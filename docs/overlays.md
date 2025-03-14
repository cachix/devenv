# Overlays

!!! info "New in version 1.4.2"

Overlays in devenv allow you to modify or extend the default package set (`pkgs`) that devenv uses. This is useful when you need to:

- Override existing packages to apply patches
- Add new packages that aren't in the default set
- Use custom builds of existing packages

## Using overlays

To add overlays to your devenv configuration, use the `overlays` option in your `devenv.nix` file:

```nix
{ pkgs, ... }:

{
  # List of overlays to apply to pkgs
  overlays = [
    # Each overlay is a function that takes two arguments: final and prev
    (final: prev: {
      # Override an existing package
      hello = prev.hello.overrideAttrs (oldAttrs: {
        patches = (oldAttrs.patches or []) ++ [ ./hello-fix.patch ];
      });

      # Add a custom package
      my-custom-package = final.callPackage ./my-package.nix {};
    })
  ];

  # Now you can use the modified or added packages
  packages = [ pkgs.hello pkgs.my-custom-package ];
}
```

## How overlays work

Each overlay is a function that takes two arguments:
- `final`: The final package set after all overlays are applied
- `prev`: The package set as it existed before this overlay (but after previous overlays)

The function should return an attrset containing the packages you want to add or modify. These will be merged into the final package set.

## Common use cases

### Patching existing packages

```nix
overlays = [
  (final: prev: {
    # Apply a patch to fix a bug
    somePackage = prev.somePackage.overrideAttrs (oldAttrs: {
      patches = (oldAttrs.patches or []) ++ [ ./my-fix.patch ];
    });
  })
];
```

### Using a different version of a package

```nix
overlays = [
  (final: prev: {
    # Use a specific version of Node.js
    nodejs = prev.nodejs-18_x;
  })
];
```

### Adding custom packages

```nix
overlays = [
  (final: prev: {
    # Add a package from a local derivation
    my-tool = final.callPackage ./nix/my-tool.nix {};
  })
];
```

### Using packages from older nixpkgs

First, add the extra input to your `devenv.yaml`:

```yaml
inputs:
  nixpkgs:
    url: github:cachix/devenv-nixpkgs/rolling
  nixpkgs-unstable:
    url: github:nixos/nixpkgs/nixpkgs-unstable
```

Then use it in your `devenv.nix`:

```nix
{ pkgs, inputs, ... }:

{
  overlays = [
    (final: prev: {
      # Use a package from nixpkgs-unstable
      nodejs = (import inputs.nixpkgs-unstable {
        system = prev.stdenv.system;
      }).nodejs;
    })
  ];

  # Now you can use these packages from your regular pkgs
  languages.javascript.enable = true;
}
```
