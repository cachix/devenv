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

!!! note "YAML composition"

    Local `devenv.yaml` composition was added in 1.10. Remote input composition
    is supported in 2.2.1 and later.

## Sharing configuration from another repository

To keep your devenv configuration in a separate repository, declare it as an
input and import the directory containing its `devenv.yaml` and `devenv.nix`:

```yaml title="devenv.yaml"
inputs:
  shared-config:
    url: github:my-org/shared-devenv-config
    flake: false
imports:
- shared-config/profiles/backend
```

This uses the existing input import syntax; no additional YAML option is
required. Imports inside the remote `devenv.yaml` are composed recursively,
and relative `path:` inputs declared there resolve inside the fetched source.

The importing project's `devenv.lock` is the single lock file for the composed
environment. A `devenv.lock` in the imported repository is not merged. Input
declarations use the same precedence as local YAML composition, so a declaration
in the root project wins when both configurations use the same name. In
particular, when the shared configuration also declares `nixpkgs`, the root
project's pinned `nixpkgs` is reused instead of creating a second one. Use
distinct input names when the versions must remain independent.

For a sibling checkout, use `url: path:../shared-config/` in the same example.
The shared repository may also contain only a `devenv.nix` file.
Combine this with [profiles](profiles.md) to define one shared configuration that adapts to each project.

!!! tip "New in version 2.2"

    Changes to files in local `path:` inputs are picked up automatically.
    Previously the evaluation cache held on to the old configuration until `.devenv` was deleted.

See [devenv.yaml reference](reference/yaml-options.md#imports) for all supported import options.
