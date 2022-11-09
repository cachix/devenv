If you omit `devenv.yaml`, it defaults to:

```yaml title="devenv.yaml"
inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  pre-commit-hooks:
    url: github:cachix/pre-commit-hooks.nix
```

Input name like ``nixpkgs`` and ``pre-commit-hooks`` are identifiers for what
 is passsed in the first ine of the function:

```nix title="devenv.nix"
{ pkgs, lib, nixpkgs, pre-commit-hooks, config, ... }:

{
}
```

There are a few special inputs:

- ``pkgs`` is ``nixpkgs`` input resolved for your platform and contains all the packages.
- ``lib`` is [a collection of functions that help manipulate basic data structures](https://nixos.org/manual/nixpkgs/stable/#sec-functions-library).
- ``config`` is the resolved configuration if the developer environment, so you can reference any other option set in your ``devenv.nix``.
- ``pre-commit-hooks`` is already wired up by default as an import so you can [set up your hooks](pre-commit-hooks.md).


!!! note

    If you're not referencing an input, you can leave it out from the function arguments as ``...`` catches all the inputs you're not interested in.


See [devenv.yaml reference](reference/yaml-options.md#inputs) for all supported inputs.

# Locking and updating inputs

When you run any of the commands,
``devenv`` will resolve inputs like ``github:NixOS/nixpkgs/nixpkgs-unstable`` into a commit revision and write all that to ``devenv.lock`. This ensures that your environment is reproducible.

To update to a newer commit run ``devenv update`` or read [devenv.yaml reference](reference/yaml-options.md#inputs) how to pin down the revision/branch at the input level.

