Inputs allow you to refer to Nix code outside your project,
while preserving reproducability. 

Think of inputs as dependency management of your developer environment.

If you omit `devenv.yaml`, it defaults to:

```yaml title="devenv.yaml"
inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  pre-commit-hooks:
    url: github:cachix/pre-commit-hooks.nix
```

Input name like ``nixpkgs`` and ``pre-commit-hooks`` are identifiers for what
 is passsed in the first line of the function:

```nix title="devenv.nix"
{ pkgs, lib, nixpkgs, pre-commit-hooks, config, ... }:

{
}
```

See [basics](basics.md) for more about ``devenv.nix``.

There are a few special inputs pass into ``devnix.nix``:

- ``pkgs`` is ``nixpkgs`` input resolved for your platform and contains all the packages.
- ``lib`` is [a collection of functions that help manipulate basic data structures](https://nixos.org/manual/nixpkgs/stable/#sec-functions-library).
- ``config`` is the resolved configuration of the developer environment, so you can reference any other option set in your ``devenv.nix``.
- ``pre-commit-hooks`` is already wired up by default as an import so you can [set up your hooks](pre-commit-hooks.md).


!!! note

    If you're not referencing an input, you can leave it out from the function arguments as ``...`` catches all the inputs you're not interested in.


See [devenv.yaml reference](reference/yaml-options.md#inputs) for all supported inputs.

## Locking and updating inputs

When you run any of the commands,
``devenv`` will resolve inputs like ``github:NixOS/nixpkgs/nixpkgs-unstable`` into a commit revision and write all that to ``devenv.lock``. This ensures that your environment is reproducible.

To update to a newer commit run ``devenv update`` or read [devenv.yaml reference](reference/yaml-options.md#inputs) how to pin down the revision/branch at the input level.

