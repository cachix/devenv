Here are the minimal steps to get started.

## Installation


1. Install [Nix](https://nixos.org)

    ```shell-session
    sh <(curl -L https://nixos.org/nix/install)
    ```

2. Install ``devenv``

=== "Newcomers"

    ```shell-session
    nix-env -if https://github.com/cachix/devenv/tarball/v0.1
    ```

=== "Newcomers (flakes)"

    ```shell-session
    nix profile install github:cachix/devenv/v0.1
    ```

=== "Declaratively (non-flakes)"
    
    ```nix
    (import (fetchTarball https://github.com/cachix/devenv/archive/v0.1.tar.gz))
    ```

=== "Declaratively (flakes)"

    ```nix
    inputs.devenv.url = github:cachix/devenv/v0.1;
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

- ``devenv ci`` builds your developer environment and makes sure that all checks pass. Useful to run in your Continuous Integration environment.
- ``devenv shell`` activates your developer environment.
- ``devenv update`` updates and pins inputs from ``devenv.yaml`` into ``devenv.lock``.
- ``devenv gc`` [deletes unused environments](garbage-collection.md) to save disk space.
- ``devenv up`` starts [processes](processes.md).

## Learn more

- About ``.envrc`` in [Automatic Shell Activation](automatic-shell-activation.md).
- About ``devenv.yaml`` in [Inputs](inputs.md) and [Composing Using Imports](composing-using-imports.md).
- About ``devenv.nix`` in the **Writing devenv.nix** section, starting with [the Basics](basics.md).
