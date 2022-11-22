Inputs allow you to refer to Nix code outside of your project
while preserving reproducibility.

Think of inputs as dependency management for your developer environment.

If you omit `devenv.yaml`, it defaults to:

```yaml title="devenv.yaml"
inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  pre-commit-hooks:
    url: github:cachix/pre-commit-hooks.nix
```

The dependencies you mention as `inputs` are passsed as an argument to the function.

```nix title="devenv.nix"
{ inputs, ... }:

let
  pre-commit-check = inputs.pre-commit-hooks.run {
    src = ./.;
    hooks.shellcheck.enable = true;
  };
in
{
  enterShell = ''
    ${pre-commit-check.shellHook}
  ''
}
```

See [basics](basics.md) for more about ``devenv.nix``.

There are a few special inputs passed into ``devenv.nix``:

```nix title="devenv.nix"
{ pkgs, lib, config, ... }:

{
}
```

- ``pkgs`` is a ``nixpkgs`` input containing all of the available packages for your system.
- ``lib`` is [a collection of functions for working with Nix data structures](https://nixos.org/manual/nixpkgs/stable/#sec-functions-library).
- ``config`` is the resolved configuration for your developer environment, which you can use to reference any other options set in ``devenv.nix``.


!!! note

    ``...`` is a catch-all pattern for any additional inputs, so you can safely omit the inputs you're not using.


See [devenv.yaml reference](reference/yaml-options.md#inputs) for all supported inputs.

## Locking and updating inputs

When you run any of the commands,
``devenv`` resolves inputs like ``github:NixOS/nixpkgs/nixpkgs-unstable`` into a commit revision and writes it to ``devenv.lock``. This ensures that your environment is reproducible.

To update an input to a newer commit, run ``devenv update`` or read [devenv.yaml reference](reference/yaml-options.md#inputs) to learn how to pin down the revision/branch at the input level.

