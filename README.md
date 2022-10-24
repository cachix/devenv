# devenv - Fast, Declarative, Reproducible, Composable Developer Environments

See [Nix language tutorial](https://nix.dev/tutorials/nix-language) for a primer.

Given `devenv`:

```nix
{ pkgs, ... }:

{
  env.FOO = true;

  include = [ ./frontend/devenv ];

  enter = ''
    echo hello
  '';

  packages = [ pkgs.git ];

  processes."<name>".cmd = "lala";
}
```

And `dev.yaml`:

```yaml
inputs:
  - nixpkgs:
     - url: github:NixOS/nixpkgs/nixos-22.05
```

## Commands

``devenv shell``: make `packages` available and export `env` variables

``devenv up``: start all `processes`

``devenv init``: generate `devenv`, `dev.yaml` and `.envrc`

``devenv update``: bump `dev.lock`

``devenv gc``: remove old shells


## Installation

  $ install nix
  $ nix-env -if https://github.com/cachix/devenv/tarball/master

## TODO

- integrations via flakes
- postgres module
- cachix integration: when composing as well
- pre-commit.nix integration
- registry of devenv modules
- implement a bunch of simple options via yaml
- top 10 most used languages support

## Roadmap

- support for building containers in a fast way
