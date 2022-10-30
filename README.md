# devenv.sh - Fast, Declarative, Reproducible, and Composable Developer Environments

[![](https://dcbadge.vercel.app/api/server/naMgvexb6q)](https://discord.gg/naMgvexb6q)
[![CI](https://github.com/cachix/devenv.sh/actions/workflows/buildtest.yml/badge.svg)](https://github.com/cachix/devenv.sh/actions/workflows/buildtest.yml)

See [Nix language tutorial](https://nix.dev/tutorials/nix-language) for a primer.

Given `devenv.nix`:

```nix
{ pkgs, ... }:

{
  env.FOO = true;

  enterShell = ''
    echo hello
  '';

  packages = [ pkgs.git ];

  processes."<name>".exec = "lala";
}
```

And `devenv.yaml`:

```yaml
inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixos-22.05
imports:
  - ./frontend
  - ./backend
```

## Commands

``devenv init``: generate `devenv.nix`, `devenv.yaml` and `.envrc`

``devenv shell``: make `packages` available and export `env` variables

``devenv up``: start all `processes`

``devenv update``: bump `devenv.lock`

``devenv gc``: remove old shells

## Benefits

### Fast

### Declarative

### Reproducible

### Composable

## Installation

1. Install [Nix](https://nixos.org)

```
    $ sh <(curl -L https://nixos.org/nix/install)
```

2. Install `devenv`

```
    $ nix-env -if https://github.com/cachix/devenv/tarball/master
```

## Usage 

XXX 

## Roadmap

- [devenv search](https://github.com/cachix/devenv.sh/issues/4)
- [support for building containers using https://github.com/nlewo/nix2container](https://github.com/cachix/devenv.sh/issues/5)
