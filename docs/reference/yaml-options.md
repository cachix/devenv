# devenv.yaml

| Key                                                           | Value                                                                         |
|---------------------------------------------------------------|-------------------------------------------------------------------------------|
| clean.enabled                                                 | Clean the environment when entering the shell. Defaults to `false`.           |
| clean.keep                                                    | A list of environment variables to keep when cleaning the environment.        |
| imports                                                       | A list of relative paths, absolute paths, or references to inputs to import ``devenv.nix`` and ``devenv.yaml`` files. |
| impure                                                        | Relax the hermeticity of the environment.                                     |
| inputs                                                        | Defaults to `inputs.nixpkgs.url: github:cachix/devenv-nixpkgs/rolling`.       |
| inputs.&lt;name&gt;                                           | Identifier name used when passing the input in your ``devenv.nix`` function.  |
| inputs.&lt;name&gt;.flake                                     | Does the input contain ``flake.nix`` or ``devenv.nix``. Defaults to ``true``. |
| inputs.&lt;name&gt;.follows                                       | Another input to "inherit" from by name. [Following inputs](#following-inputs).                                |
| inputs.&lt;name&gt;.inputs.&lt;name&gt;.follows                                      | Override nested inputs by name. [Supported formats](#supported-uri-formats).                                |
| inputs.&lt;name&gt;.overlays                                  | A list of overlays to include from the input.                                 |
| inputs.&lt;name&gt;.url                                       | URI specification of the input. [Supported formats](#supported-uri-formats).                                |
|                                                               |                                                                              |
| nixpkgs.allowBroken                                           | Allow packages marked as broken. Defaults to `false`.                         |
| nixpkgs.allowUnfree                                           | Allow unfree packages. Defaults to `false`.                                   |
| nixpkgs.cudaCapabilities                                      | Select CUDA capabilities for nixpkgs. Defaults to `[]`                        |
| nixpkgs.cudaSupport                                           | Enable CUDA support for nixpkgs. Defaults to `false`.                         |
| nixpkgs.permittedInsecurePackages                             | A list of insecure permitted packages. Defaults to `[]`                       |
| nixpkgs.permittedUnfreePackages                               | A list of unfree packages to allow by name. Defaults to `[]`                  |
|                                                               |                                                                               |
| nixpkgs.per-platform.&lt;system&gt;.allowBroken               | (per-platform) Allow packages marked as broken. Defaults to `false`.          |
| nixpkgs.per-platform.&lt;system&gt;.allowUnfree               | (per-platform) Allow unfree packages. Defaults to `false`.                    |
| nixpkgs.per-platform.&lt;system&gt;.cudaCapabilities          | (per-platform) Select CUDA capabilities for nixpkgs. Defaults to `[]`         |
| nixpkgs.per-platform.&lt;system&gt;.cudaSupport               | (per-platform) Enable CUDA support for nixpkgs. Defaults to `false`.          |
| nixpkgs.per-platform.&lt;system&gt;.permittedInsecurePackages | (per-platform) A list of insecure permitted packages. Defaults to `[]`        |
| nixpkgs.per-platform.&lt;system&gt;.permittedUnfreePackages   | (per-platform) A list of unfree packages to allow by name. Defaults to `[]`   |
|                                                               |                                                                               |
| secretspec.enable                                             | Enable [secretspec integration](../integrations/secretspec.md). Defaults to `false`.                           |
| secretspec.profile                                            | Secretspec profile name to use.                                               |
| secretspec.provider                                           | Secretspec provider to use.                                                   |

!!! note "Added in 1.9"

    - nixpkgs.permittedUnfreePackages

!!! note "Added in 1.8"

    - `secretspec`

!!! note "Added in 1.7"

    - `nixpkgs`

!!! note "Added in 1.0"

    - relative file support in imports: `./mymodule.nix`
    - `clean`
    - `impure`
    - `allowBroken`

## Inputs

### Supported URI formats

`inputs.<name>.url` is a URI format that allows importing external repositories, files, directories, and more as inputs to your development environment.

devenv supports the same URI specification for inputs as Nix Flakes.

For a more detailed description of the supported URI formats, see the [Nix manual](<https://nix.dev/manual/nix/latest/command-ref/new-cli/nix3-flake.html#types>).

We'll list the most common examples below.

#### GitHub

- `github:NixOS/nixpkgs/master`
- `github:NixOS/nixpkgs?rev=238b18d7b2c8239f676358634bfb32693d3706f3`
- `github:org/repo?dir=subdir`
- `github:org/repo?tag=v1.0.0`

#### GitLab

- `gitlab:owner/repo/branch`
- `gitlab:owner/repo/commit`
- `gitlab:owner/repo?host=git.example.org`

#### Git repositories

- `git+ssh://git@github.com/NixOS/nix?ref=v1.2.3`
- `git+https://git.somehost.tld/user/path?ref=branch&rev=fdc8ef970de2b4634e1b3dca296e1ed918459a9e`
- `git+file:///some/absolute/path/to/repo`

#### Mercurial

- `hg+https://...`
- `hg+ssh://...`
- `hg+file://...`

#### Sourcehut

- `sourcehut:~misterio/nix-colors/21c1a380a6915d890d408e9f22203436a35bb2de?host=hg.sr.ht`

#### Tarballs

- `tarball+https://example.com/foobar.tar.gz`

#### Local files

Path inputs don't respect `.gitignore` and will copy the entire directory to the Nix store.
To avoid unnecessarily copying large development directories, consider using `git+file` instead.

- `path:/path/to/repo`
- `file+https://`
- `file:///some/absolute/file.tar.gz`

### Following inputs

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

## An extensive example

```yaml
allowUnfree: true
allowBroken: true
clean:
  enabled: true
  keep:
    - EDITOR
inputs:
  nixpkgs:
    url: github:cachix/devenv-nixpkgs/rolling
  myproject:
    url: github:owner/myproject
    flake: false
  myproject2:
    url: github:owner/myproject
    overlays:
      - default
imports:
  - ./frontend
  - ./backend
  - ./mymodule.nix
  - /absolute/path/from/git/root
  - myproject
  - myproject/relative/path
```

!!! note "Added in 1.10"

    - local imports now merge `devenv.yaml` (remote inputs not yet supported)
    - absolute path support in imports: `/absolute/path/from/git/root`

!!! note "Added in 1.0"

    - relative file support in imports: `./mymodule.nix`

## Using permittedUnfreePackages

Instead of allowing all unfree packages with `nixpkgs.allowUnfree: true`, you can selectively permit specific unfree packages by name:

```yaml
# Use the nixpkgs-scoped configuration
nixpkgs:
  permittedUnfreePackages:
    - terraform
    - vscode

# Or configure per-platform
nixpkgs:
  per-platform:
    x86_64-linux:
      permittedUnfreePackages:
        - some-package
    aarch64-darwin:
      permittedUnfreePackages:
        - some-package
```
