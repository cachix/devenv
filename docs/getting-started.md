
## Installation


### 1. Install [Nix](https://nixos.org)

=== "Linux"

    ```
    sh <(curl -L https://nixos.org/nix/install) --daemon
    ```

=== "macOS"

    ```
    curl -L https://raw.githubusercontent.com/NixOS/experimental-nix-installer/main/nix-installer.sh | sh -s install
    ```

    !!! note "Experimental installer"
        We recommend using the above experimental installer.
        It can handle OS upgrades and has better support for Apple silicon.

        If you'd like to stick with the official release installer, use:
        ```
        sh <(curl -L https://nixos.org/nix/install)
        ```

    **Upgrade Bash**

    macOS ships with an ancient version of Bash due to licensing reasons.

    We recommend installing a newer version from nixpkgs to avoid running into evaluation errors.

    === "Newcomers"

        ```
        nix-env -iA bashInteractive -f https://github.com/NixOS/nixpkgs/tarball/nixpkgs-unstable
        ```

    === "Nix profiles (requires experimental flags)"

        ```
        nix profile install nixpkgs#bashInteractive
        ```

=== "Windows (WSL2)"

    ```
    sh <(curl -L https://nixos.org/nix/install) --no-daemon
    ```

=== "Docker"

    ```
    docker run -it nixos/nix
    ```


### 2. Install [devenv](https://github.com/cachix/devenv)


=== "Newcomers"

    ```
    nix-env -iA devenv -f https://github.com/NixOS/nixpkgs/tarball/nixpkgs-unstable
    ```

=== "Nix profiles (requires experimental flags)"

    ```
    nix profile install nixpkgs#devenv
    ```

=== "NixOS/nix-darwin/home-manager"

    ```nix title="configuration.nix"
    environment.systemPackages = [ 
      pkgs.devenv
    ];
    ```


!!! Updating

    To update, refer to the specific upgrade instructions provided in the documentation for the installer you used from the options above. 


## Initial set up

Given a Git repository, create the initial structure:

```shell-session
$ devenv init
• Creating .envrc
• Creating devenv.nix
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
