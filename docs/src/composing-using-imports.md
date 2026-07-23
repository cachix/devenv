# Composing using imports

You can compose environments either locally or by referencing [inputs](inputs.md).

Imagine you're building a typical web application, with separate frontend and backend components
that live in separate folders.

```yaml title="devenv.yaml"
inputs:
  nixpkgs:
    url: github:cachix/devenv-nixpkgs/rolling
  devenv:
    url: github:cachix/devenv
    flake: false
imports:
- ./frontend
- ./backend
- devenv/examples/supported-languages
- devenv/examples/scripts
```

If you enter the ``frontend`` directory, the environment will activate based on what's in the ``frontend/devenv.nix`` file.

If you enter the top-level project, the environment is combined with what's defined in ``backend/devenv.nix`` and ``frontend/devenv.nix``.
For example, ``devenv up`` will start both the frontend and backend processes.

!!! note "Added in 1.10"

    Composing ``devenv.yaml`` files is now supported for local files (relative and absolute paths).
    Remote inputs are not yet supported for ``devenv.yaml`` imports.

## Sharing configuration from another repository

To keep your devenv configuration in a separate repository, for example when working on a team that doesn't use devenv, declare it as a `path:` input and import it:

```yaml title="devenv.yaml"
inputs:
  shared-config:
    url: path:../shared-config/
    flake: false
imports:
- shared-config
```

The sibling `shared-config` repository only needs a `devenv.nix` file.
Combine this with [profiles](profiles.md) to define one shared configuration that adapts to each project.

!!! tip "New in version 2.2"

    Changes to files in local `path:` inputs are picked up automatically.
    Previously the evaluation cache held on to the old configuration until `.devenv` was deleted.

See [devenv.yaml reference](reference/yaml-options.md#imports) for all supported import options.
