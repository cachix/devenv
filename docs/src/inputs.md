Inputs allow you to refer to Nix code outside of your project
while preserving reproducibility.

Think of inputs as dependency management for your developer environment.

If you omit `devenv.yaml`, it defaults to:

```yaml title="devenv.yaml"
inputs:
  nixpkgs:
    url: github:cachix/devenv-nixpkgs/rolling
  git-hooks:
    url: github:cachix/git-hooks.nix
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

See [devenv.yaml reference](reference/yaml-options.md) for all supported input options.

## Supported URI formats

`inputs.<name>.url` is a URI format that allows importing external repositories, files, directories, and more as inputs to your development environment.

devenv supports the same URI specification for inputs as Nix Flakes.

For a more detailed description of the supported URI formats, see the [Nix manual](<https://nix.dev/manual/nix/latest/command-ref/new-cli/nix3-flake.html#types>).

We'll list the most common examples below.

### GitHub

- `github:NixOS/nixpkgs/master`
- `github:NixOS/nixpkgs?rev=238b18d7b2c8239f676358634bfb32693d3706f3`
- `github:org/repo?dir=subdir`
- `github:org/repo?ref=v1.0.0`

### GitLab

- `gitlab:owner/repo/branch`
- `gitlab:owner/repo/commit`
- `gitlab:owner/repo?host=git.example.org`

### Git repositories

- `git+ssh://git@github.com/NixOS/nix?ref=v1.2.3`
- `git+https://git.somehost.tld/user/path?ref=branch&rev=fdc8ef970de2b4634e1b3dca296e1ed918459a9e`
- `git+file:///some/absolute/path/to/repo`

### Mercurial

- `hg+https://...`
- `hg+ssh://...`
- `hg+file://...`

### Sourcehut

- `sourcehut:~misterio/nix-colors/21c1a380a6915d890d408e9f22203436a35bb2de?host=hg.sr.ht`

### Tarballs

- `tarball+https://example.com/foobar.tar.gz`

### Local files

Path inputs don't respect `.gitignore` and will copy the entire directory to the Nix store.
To avoid unnecessarily copying large development directories, consider using `git+file` instead.

- `path:/path/to/repo`
- `file+https://`
- `file:///some/absolute/file.tar.gz`

## Following inputs

Inputs can also "follow" other inputs by name.

The two main use-cases for this are to:

- Inherit inputs from other `devenv.yaml`s or external flake projects.
- Reduce the number of repeated inputs that need to be downloaded by overriding nested inputs.

`follows` are specified by name. Nested inputs can be referenced by name using `/` as a separator.

For example, to use a `nixpkgs` input from a shared `base-project` input:

```yaml hl_lines="5"
inputs:
  base-project:
    url: github:owner/repo
  nixpkgs:
    follows: base-project/nixpkgs
```

Or to override the `nixpkgs` input of another input to reduce the number of times `nixpkgs` has to be downloaded:

```yaml hl_lines="6-8"
inputs:
  nixpkgs:
    url: github:cachix/devenv-nixpkgs/rolling
  git-hooks:
    url: github:cachix/git-hooks.nix
    inputs:
      nixpkgs:
        follows: nixpkgs
```

## Locking and updating inputs

When you run any of the commands, `devenv` resolves inputs like `github:NixOS/nixpkgs/nixpkgs-unstable` into a commit revision and writes them to `devenv.lock`. This ensures that your environment is reproducible.

To update an input to a newer commit, run `devenv update` or read the [devenv.yaml reference](reference/yaml-options.md) to learn how to pin down the revision/branch at the input level.
