Here are the minimal steps to get started.

## Installation


1. Install [Nix](https://nixos.org)

```shell-session
sh <(curl -L https://nixos.org/nix/install)
```

2. Install [devenv](https://github.com/cachix/devenv)

```shell-session
nix-env -if https://devenv.sh/assets/devenv-preview.tar.gz
```

*This might take a few minutes, please bear with us until we provide binaries.*

## Initial setup

Given a git repository, create the initial structure:

```shell-session
$ devenv init
Creating .envrc
Creating devenv.nix
Creating devenv.yaml
Appending .devenv* to .gitignore
Done.
```

## Commands

- ``devenv ci`` builds your developer environment and make sure all checks pass. Useful to run on your Continuous Integration.
- ``devenv shell`` activates your developer environment.
- ``devenv update`` updates and pins inputs from ``devenv.yaml`` into ``devenv.lock``.
- ``devenv gc`` [deletes unused environments](garbage-collection.md) to save disk space.
- ``devenv up`` starts [processes](processes.md).

## Learn more

- About ``.envrc`` in [Automatic Shell Activation](automatic-shell-activation.md).
- About ``devenv.yaml`` in [Inputs](inputs.md) and [Composing Using Imports](composing-using-imports.md).
- About ``devenv.nix`` in **Writing devenv.nix** section, starting with [the Basics](basics.md).
