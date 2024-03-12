Inputs allow you to refer to Nix code outside of your project
while preserving reproducibility.

Think of inputs as dependency management for your developer environment.

If you omit `devenv.yaml`, it defaults to:

```yaml title="devenv.yaml"
inputs:
  nixpkgs:
    url: github:cachix/devenv-nixpkgs/rolling
  pre-commit-hooks:
    url: github:cachix/pre-commit-hooks.nix
```

The dependencies you mention as `inputs` are passed as an argument to the function.

For example, if you have a `devenv.yaml` file like:

```yaml title="devenv.yaml"
inputs:
  nixpkgs-stable:
    url: github:NixOS/nixpkgs/nixos-23.11
```

You can access the stable packages via the `inputs` field:

```nix title="devenv.nix"
{ inputs, pkgs, ... }:

let
  pkgs-stable = import inputs.nixpkgs-stable { system = pkgs.stdenv.system; };
in {
  packages = [ pkgs-stable.git ];

  enterShell = ''
    git --version
  ''
}
```

See [basics](basics.md) for more about `devenv.nix`.

There are a few special inputs passed into `devenv.nix`:

```nix title="devenv.nix"
{ pkgs, lib, config, ... }:

{
  env.GREET = "hello";

  enterShell = ''
    echo ${config.env.GREET}
  '';
}
```

- `pkgs` is a `nixpkgs` input containing [all of the available packages](./packages.md#searching) for your system.
- `lib` is [a collection of functions for working with Nix data structures](https://nixos.org/manual/nixpkgs/stable/#sec-functions-library). You can use [noogle](https://noogle.dev/) to search for a function.
- `config` is the final resolved configuration for your developer environment, which you can use to reference any other options set in [devenv.nix](./reference/options.md). 
   Since Nix supports lazy evaluation, you can reference any option you define in the same file as long as it doesn't reference itself!

!!! note

    ``...`` is a catch-all pattern for any additional inputs, so you can safely omit the inputs you're not using.

See [devenv.yaml reference](reference/yaml-options.md#inputs) for all supported inputs.

## Locking and updating inputs

When you run any of the commands, `devenv` resolves inputs like `github:NixOS/nixpkgs/nixpkgs-unstable` into a commit revision and writes them to `devenv.lock`. This ensures that your environment is reproducible.

To update an input to a newer commit, run `devenv update` or read the [devenv.yaml reference](reference/yaml-options.md#inputs) to learn how to pin down the revision/branch at the input level.
