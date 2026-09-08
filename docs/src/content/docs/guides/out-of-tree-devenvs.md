---
title: "Out-of-tree devenvs"
description: "Use a devenv configuration from another local or remote repository."
---

<small class="added-in">Added in <code>2.0</code></small>

An out-of-tree devenv lets a project use a configuration that exists outside that project.
This method lets many repositories share one configuration without copies.
Your project does not need a local `devenv.nix`.

## Select a source

The `--from` option accepts a local path or a [supported input URI](/inputs/#supported-uri-formats):

```console
$ devenv --from path:../shared-devenv shell
$ devenv --from 'github:myorg/devenv-configs?dir=rust-web' shell
```

The `path:` prefix is required for file system paths.
devenv resolves a relative path from the directory where you run the command.
The `dir` query selects a devenv within the fetched repository.

Use `--from` with commands such as `shell`, `test`, `up`, and `build`.

:::caution
The source can define scripts, processes, and shell hooks.
Use only a source that you trust.
:::

## Bind a directory to the source

Use `allow` to save the source for the current directory:

```console
$ cd my-project
$ devenv --from 'github:myorg/devenv-configs?dir=rust-web' allow
$ devenv shell
```

The binding applies to the current directory and its subdirectories.
Later commands use the saved source without `--from`.
An installed [shell hook](/auto-activation/) also starts the environment when you enter the bound directory.

:::tip[Persistent bindings in version 2.2]
Before version 2.2, each command required the `--from` option.
:::

## Load the source configuration

An out-of-tree source can contain these files and references:

- `devenv.nix` and `devenv.local.nix`
- `devenv.yaml` and `devenv.local.yaml`
- YAML imports and inputs
- Nix modules from the YAML import graph

:::tip[Complete fetched sources in version 2.3.1]
Fetched sources now load the complete YAML configuration.
Earlier versions loaded only `devenv.nix` from a fetched source.
:::

A local `path:` source stays live.
devenv reads its changes without another fetch or `allow` command.

A fetched source uses the revision in the target directory's `devenv.lock`.
Run `devenv update` to fetch a new revision.
Commit the lock file to give all users the same source revision.

## Add target-specific YAML

The target directory can contain `devenv.yaml` and `devenv.local.yaml` for project-specific settings.
devenv loads the source YAML first, so the target YAML has higher precedence.

An explicit `--from` option does not load the target's `devenv.nix` or `devenv.local.nix`.
Without an explicit option, a local `devenv.nix` has priority over a saved binding.

## Save profiles

Pass profiles to `allow` to save them with the binding:

```console
$ devenv --from github:myorg/devenv-configs \
    --profile backend \
    --profile observability \
    allow
```

devenv applies these profiles to later commands.
An explicit `--profile` option has priority over the saved profiles.

## Change or remove the binding

Run `allow` with a different source to replace the current binding:

```console
$ devenv --from path:../new-shared-devenv allow
```

Run `revoke` to remove the binding and its trust entry:

```console
$ devenv revoke
```
