# devenv - Fast, Declarative, Reproducible, and Composable Developer Environments

See [Nix language tutorial](https://nix.dev/tutorials/nix-language) for a primer.

Given `devenv.nix`:

```nix
{ pkgs, ... }:

{
  env.FOO = true;

  include = [ ./frontend ];

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
```

## Commands

``devenv init``: generate `devenv.nix`, `devenv.yaml` and `.envrc`

``devenv shell``: make `packages` available and export `env` variables

``devenv up``: start all `processes`

``devenv update``: bump `devenv.lock`

``devenv gc``: remove old shells


## Installation

  $ install nix
  $ nix-env -if https://github.com/cachix/devenv/tarball/master


## Roadmap

- ``devenv search``
- support for building containers in a fast way
