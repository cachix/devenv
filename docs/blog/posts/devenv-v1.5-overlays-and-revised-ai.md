---
draft: false
date: 2025-03-18
authors:
  - domenkozar
---

# devenv 1.5: Overlays and Improved AI Generation

[Last month](/blog/posts/devenv-v1.4-generating-nix-developer-environments-using-ai), we introduced AI-powered generation of Nix developer environments.

In this release, we're focusing on two key areas:

1. **Overlays**: A powerful Nix concept for modifying and extending the nixpkgs package set
2. **Improved AI Generation**: Better transparency and control over telemetry

## Overlays: Customizing Your Package Set

Overlays allow you to modify or extend the default package set (`pkgs`) that devenv uses. This is particularly useful when you need to:

- Apply patches to existing packages
- Use different versions of packages than what's provided by default
- Add custom packages not available in nixpkgs
- Use packages from older nixpkgs versions

Here's a simple example of using overlays in your `devenv.nix` file:

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

    # Add a custom package
    (final: prev: {
      my-tool = final.callPackage ./nix/my-tool.nix {};
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
      # Use Node.js from nixpkgs-unstable
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

## Improved AI Generation with Better Transparency

In order to understand how to improve the AI generation, we collect telemetry information
what kind of environments are being generated given the input.

We've [heard your feedback that telemetry collection should be explicit](https://github.com/cachix/devenv/issues/1733),
we've made `devenv generate` a separate binary so you can opt-in if you'd like to use
it and also made it warn on first use per project:

```
$ devenv generate
devenv-generate was not found in your $PATH, please install it.

Since we're collecting telemetry in order to improve the AI results,
the AI generation now ships as a separate command.
```

Once you install `devenv-generate`, we provide clearer information about telemetry collection and require explicit consent:

```
$ devenv generate a Python project using Torch
generate collects anonymous usage data to improve recommendations.
  To disable telemetry, use --disable-telemetry or set DO_NOT_TRACK=1
Do you want to continue? [Y/n]
```

Additionally we warn when generating from source code:

```
$ devenv generate
Going to upload source code using `git ls-files` to https://devenv.new to analyze the environment using AI...
Do you want to continue? [Y/n]
```

This warning is shown once per project and it remembers your choice.

We aren't going to train model on your data
and we'll review all the telemetry information case by case and use it to tweak the instructions sent to the AI model.

Join our [Discord](https://discord.gg/naMgQehY) to share feedback and suggestions!

Domen
