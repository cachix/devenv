---
draft: false
date: 2025-04-13
authors:
  - domenkozar
---

# devenv 1.5: Overlays Support and Performance Improvements

In this release, we're introducing a powerful Nix concept: overlays for modifying and extending the nixpkgs package set, along with significant performance and TLS certificate improvements.

## Overlays: Customizing Your Package Set

Overlays allow you to modify or extend the default package set (`pkgs`) that devenv uses. This is particularly useful when you need to:

- Apply patches to existing packages
- Use different versions of packages than what's provided by default
- Add custom packages not available in nixpkgs
- Use packages from older nixpkgs versions

Here's an example of using overlays in your `devenv.nix` file to apply a patch to the `hello` package:

```nix
{ pkgs, ... }:

{
  # Define overlays to modify the package set
  overlays = [
    # Override an existing package with a patch
    (final: prev: {
      hello = prev.hello.overrideAttrs (oldAttrs: {
        patches = (oldAttrs.patches or []) ++ [ ./hello-fix.patch ];
      });
    })
  ];

  # Use the modified packages
  packages = [ pkgs.hello pkgs.my-tool ];
}
```

### Using packages from a different nixpkgs version

You can even use packages from a different nixpkgs version by adding an extra input to your `devenv.yaml`:

```yaml
inputs:
  nixpkgs:
    url: github:cachix/devenv-nixpkgs/rolling
  nixpkgs-unstable:
    url: github:nixos/nixpkgs/nixpkgs-unstable
```

And then using it in your `devenv.nix`:

```nix
{ pkgs, inputs, ... }:

{
  overlays = [
    (final: prev: {
      nodejs = (import inputs.nixpkgs-unstable {
        system = prev.stdenv.system;
      }).nodejs;
    })
  ];

  # Now you can use the unstable version of Node.js
  languages.javascript.enable = true;
}
```

For more details and examples, check out the [overlays documentation](/overlays/).

## TLS Improvements: Native System Certificates

We've heard from ZScaler how [they are using devenv](https://bsky.app/profile/jm2dev.bsky.social/post/3lle7mdguhs2j) and we've fixed their major annoyance
by ensuring devenv now respects system certificates that many enterprises rely on.

## macOS Development Enhancements: Custom Apple SDK Support

For macOS developers, we've added the ability to customize which Apple SDK is used for development:

```nix
{ pkgs, ... }:

{
  apple.sdk.package = pkgs.darwin.apple_sdk.sdk;
}
```

This allows you to:
- Control exactly which version of the SDK to use
- Ensure consistency across development environments
- Avoid incompatibilities between different macOS versions

## Performance Improvements

Sander further tweaked the [performance of developer environment activation at OceanSprint](https://oceansprint.org/reports/2025/) when it can be cached:

* Linux: ~500ms -> ~150ms
* macOS: ~1300ms -> ~300ms


Join our [Discord](https://discord.gg/naMgQehY) to share feedback and suggestions!

Domen
