
## Installation


### 1. Install [Nix](https://nixos.org)

=== "Linux"

    ```
    sh <(curl -L https://nixos.org/nix/install) --daemon
    ```

=== "macOS"

    ```
    curl -L https://github.com/NixOS/experimental-nix-installer/releases/download/0.27.0/nix-installer.sh | sh -s -- install
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

    === "Nix env (newcomers)"

        ```
        nix-env --install --attr bashInteractive -f https://github.com/NixOS/nixpkgs/tarball/nixpkgs-unstable
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
    nix-env --install --attr devenv -f https://github.com/NixOS/nixpkgs/tarball/nixpkgs-unstable
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

### 3. Configure a GitHub access token (optional)

The Nix ecosystem is heavily dependent on GitHub for hosting and distributing source code, like the source for nixpkgs.
This means that Nix will make a lot of un-authenticated requests to the GitHub API and you may encounter rate-limiting.

To avoid being rate-limited, **we recommend providing Nix with a GitHub access token**, which will greatly increase your API limits.

Create a new token with no extra permissions at [https://github.com/settings/personal-access-tokens/new](https://github.com/settings/personal-access-tokens/new).
Add the token to your `~/.config/nix/nix.conf`:

```
access-tokens = github.com=<GITHUB_TOKEN>
```

## Initial set up

Initialize a new developer environment with `devenv init`.

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

## Updating

### Update devenv CLI

=== "Nix env (newcomers)"

    ```
    nix-env --upgrade --attr devenv -f https://github.com/NixOS/nixpkgs/tarball/nixpkgs-unstable
    ```

=== "Nix profiles (requires experimental flags)"

    ```
    nix profile upgrade devenv
    ```

=== "NixOS/nix-darwin/home-manager"

    Update nixpkgs to get the latest version of devenv.

    For detailed upgrade instructions specific to your setup, please refer to the documentation for your particular system: NixOS, nix-darwin (for macOS), or home-manager, as applicable.

### Update project inputs

Inputs, like nixpkgs and devenv modules, are downloaded and pinned in a `devenv.lock` lockfile.

These should be periodically updated with:

```
devenv update
```

Learn more about [Inputs](inputs.md).
