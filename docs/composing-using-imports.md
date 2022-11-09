You can compose environments either locally or by referencing [inputs](inputs.md).

Let's assume you are writing an application with frontend and backend folders.
For some reason you wrote your own redis integration that lives in ``devenv/devenv.nix``
of the ``https://github.com/mycompany/redis.devenv`` repository.

```yaml title="devenv.yaml"
inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  redis:
    url: github:mycompany/redis.devenv
imports:
- ./frontend
- ./backend
- redis/devenv
```

If you enter ``frontend`` directory, the environment will activate based on what's in ``frontend/devenv.nix`` file.

If you enter the top-level project, the environment is combined from what's defined in ``backend/devenv.nix`` and ``frontend/devenv.nix``. For example ``devenv up`` will start frontend and backend processes.

!!! note

    While composing ``devenv.nix`` is a key feature, 
    composing ``devenv.yaml`` [hasn't been implemented yet](https://github.com/cachix/devenv/issues/14).

See [devenv.yaml reference](reference/yaml-options.md#inputs) for all supported imports.