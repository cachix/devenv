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
imports:
- ./frontend
- ./backend
- devenv/examples/supported-languages
- devenv/examples/scripts
```

If you enter the ``frontend`` directory, the environment will activate based on what's in the ``frontend/devenv.nix`` file.

If you enter the top-level project, the environment is combined with what's defined in ``backend/devenv.nix`` and ``frontend/devenv.nix``.
For example, ``devenv up`` will start both the frontend and backend processes.

!!! note

    While composing ``devenv.nix`` is a key feature, 
    composing ``devenv.yaml`` [hasn't been implemented yet](https://github.com/cachix/devenv/issues/14).

See [devenv.yaml reference](reference/yaml-options.md#inputs) for all supported imports.
