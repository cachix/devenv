# [devenv.sh](https://devenv.sh) - Fast, Declarative, Reproducible, and Composable Developer Environments

[![Join Discord](https://img.shields.io/discord/1036369714731036712?color=7389D8&label=discord&logo=discord&logoColor=ffffff)](https://discord.gg/naMgvexb6q) 
![License: Apache 2.0](https://img.shields.io/github/license/cachix/devenv) 
[![version](https://img.shields.io/github/v/release/cachix/devenv?color=green&label=version&sort=semver)](https://github.com/cachix/devenv/releases) 
[![CI](https://github.com/cachix/devenv/actions/workflows/buildtest.yml/badge.svg)](https://github.com/cachix/devenv/actions/workflows/buildtest.yml?branch=main)

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
    url: github:NixOS/nixpkgs/nixpkgs-unstable
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
    $ nix-env -if https://github.com/cachix/devenv/tarball/main
```

## Usage 

XXX 

## Roadmap

- [devenv search](https://github.com/cachix/devenv.sh/issues/4)
- [support for building containers using https://github.com/nlewo/nix2container](https://github.com/cachix/devenv.sh/issues/5)

## Related projects

- [Home Manager](https://github.com/nix-community/home-manager) manages your home dotfiles in a similar manner
- [nix-darwin](https://github.com/LnL7/nix-darwin) manages your macOS configuration in a similar manner