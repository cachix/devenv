
## Installation


### 1. Install [Nix](https://nixos.org)

=== "Linux"

    ```
    sh <(curl -L https://nixos.org/nix/install) --daemon
    ```
=== "macOS"

    ```
    sh <(curl -L https://nixos.org/nix/install)
    ```

=== "Windows (WSL2)"
   
    ```
    sh <(curl -L https://nixos.org/nix/install) --no-daemon
    ```

=== "Docker"

    ```
    docker run -it nixos/nix
    ```

!!! note

    We recommended to use experimental installer on macOS to avoid issues with Apple Scilicon chips:

    ```
    curl -L https://raw.githubusercontent.com/NixOS/experimental-nix-installer/main/nix-installer.sh | sh -s install
    ```

### 2. Install [Cachix](https://cachix.org)

Recommended, speeds up the installation by providing binaries.

=== "Newcomers"

    ```
    nix-env -iA cachix -f https://cachix.org/api/v1/install
    cachix use devenv
    ```

=== "Flake profiles"

    ```
    nix profile install nixpkgs#cachix
    cachix use devenv
    ```

### 3. Install [devenv](https://github.com/cachix/devenv)


=== "Newcomers"

    ```
    nix-env -if https://install.devenv.sh/latest
    ```

=== "Flake profiles"

    ```
    nix profile install --accept-flake-config tarball+https://install.devenv.sh/latest
    ```
=== "Declaratively using flakes"

    See [Using flakes](../guides/using-with-flakes)

=== "Declaratively without flakes"

    ```nix title="configuration.nix"
    environment.systemPackages = [ 
      (import (fetchTarball https://install.devenv.sh/latest)).default
    ];
    ```


!!! Updating

    Follow the instructions for an installation above. 

## Initial set up

Given a Git repository, create the initial structure:

```shell-session
$ devenv init
• Creating .envrc
• reating devenv.nix
• Creating devenv.yaml
• Creating .gitignore
```

## Commands

- ``devenv test`` builds your developer environment and makes sure that all checks pass. Useful to run in your continuous integration environment.
- ``devenv shell`` activates your developer environment.
- ``devenv search <NAME>`` searches packages matching NAME in Nixpkgs input.
- ``devenv update`` updates and pins inputs from ``devenv.yaml`` into ``devenv.lock``.
- ``devenv gc`` [deletes unused environments](garbage-collection.md) to save disk space.
- ``devenv up`` starts [processes](processes.md).

## Learn more

- About ``.envrc`` in [Automatic shell activation](automatic-shell-activation.md).
- About ``devenv.yaml`` in [Inputs](inputs.md) and [Composing using imports](composing-using-imports.md).
- About ``devenv.nix`` in the **Writing devenv.nix** section, starting with [the basics](basics.md).
