# Auto Activation

!!! tip "New in version 2.1"

devenv includes a built in shell hook that automatically activates your developer environment when you `cd` into a project directory. No external tools required.

## Setup

Add one line to your shell configuration file:

=== "Bash"

    ```bash title="~/.bashrc"
    eval "$(devenv hook bash)"
    ```

=== "Zsh"

    ```bash title="~/.zshrc"
    eval "$(devenv hook zsh)"
    ```

=== "Fish"

    ```fish title="~/.config/fish/config.fish"
    devenv hook fish | source
    ```

=== "Nushell"

    ```nu title="config.nu"
    devenv hook nu | save --force ~/.cache/devenv/hook.nu
    source ~/.cache/devenv/hook.nu
    ```

## Setup via the NixOS, nix-darwin, or home-manager module

!!! tip "New in version 2.2"

[NixOS](https://nixos.org/), [nix-darwin](https://github.com/nix-darwin/nix-darwin), and [home-manager](https://github.com/nix-community/home-manager) users can import the flake modules that devenv ships, which install the package and source the shell hook for every shell you have enabled — no rc-file edits required.

Add devenv as a flake input:

```nix title="flake.nix"
{
  inputs.devenv.url = "github:cachix/devenv";
}
```

=== "NixOS"

    ```nix title="configuration.nix"
    { inputs, ... }:
    {
      imports = [ inputs.devenv.nixosModules.default ];

      programs.devenv.enable = true;
    }
    ```

=== "nix-darwin"

    ```nix title="configuration.nix"
    { inputs, ... }:
    {
      imports = [ inputs.devenv.darwinModules.default ];

      programs.devenv.enable = true;
    }
    ```

=== "home-manager"

    ```nix title="home.nix"
    { inputs, ... }:
    {
      imports = [ inputs.devenv.homeManagerModules.default ];

      programs.devenv.enable = true;
    }
    ```

By default, every supported shell that you have enabled also gets the hook sourced. Disable an individual integration with:

```nix
programs.devenv.enableBashIntegration = false;
programs.devenv.enableZshIntegration = false;
programs.devenv.enableFishIntegration = false;
programs.devenv.enableNushellIntegration = false; # home-manager only
```

The exported hook scripts are also available directly under `${pkgs.devenv}/share/devenv/shell-integration/<shell>/hook.<ext>` if you want to wire them up by hand.

## Trusting a project

Before a project can auto activate, you need to explicitly trust it. This is a security measure that prevents untrusted projects from modifying your shell.

Navigate to the project directory and run:

```shell-session
$ cd ~/myproject
$ devenv allow
devenv: allowed /home/user/myproject
```

When you `cd` into the directory next time, devenv will automatically start a shell:

```shell-session
$ cd ~/myproject
(devenv) $
```

## Revoking trust

To stop a project from auto activating:

```shell-session
$ cd ~/myproject
$ devenv revoke
devenv: revoked /home/user/myproject
```

## How it works

The hook runs on every directory change and:

1. Walks up from the current directory looking for a `devenv.yaml` file.
2. Checks the trust database to verify the project was allowed.
3. If trusted, runs `devenv shell` in a subshell for that project.

If a project has not been trusted yet, you will see a message asking you to run `devenv allow`:

```
devenv: /home/user/myproject is not allowed. Run 'devenv allow' to trust this directory.
```

!!! note
    The hook only detects projects that have a `devenv.yaml` file. Projects with only `devenv.nix` (without `devenv.yaml`) are not detected.

## Automatic deactivation

When you `cd` out of the project directory (or any of its subdirectories), the devenv shell exits automatically and you return to your normal shell:

```shell-session
(devenv) $ cd ..
$
```

## Re-entry protection

The hook will not nest environments. While inside a `devenv shell`, navigating into a subdirectory of the same project keeps the current shell. Only navigating outside the project triggers deactivation.

## Comparison with direnv

| Feature | `devenv hook` | [direnv](integrations/direnv.md) |
|---|---|---|
| External dependencies | None | Requires direnv |
| Setup | One line in shell config | direnv install + `.envrc` per project |
| Trust granularity | Per project directory | Per `.envrc` file |
| Environment application | Spawns a subshell | Modifies current shell in place |
| Unloading on exit | Subshell exits automatically | direnv unloads variables |

Use `devenv hook` for a simple, dependency free setup. Use [direnv](integrations/direnv.md) if you prefer in place environment modification without a subshell.
