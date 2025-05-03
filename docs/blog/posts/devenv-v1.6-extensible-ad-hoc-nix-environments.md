---
date: 2025-04-25
authors:
  - domenkozar
draft: false
---

# devenv 1.6: Extensible Ad-Hoc Nix Environments

devenv 1.6 [has been tagged](https://github.com/cachix/devenv/releases/tag/v1.6), allowing you to:

- Create temporary environments directly from the command line without requiring a `devenv.nix` file.
- Temporarily modify existing environments.

## Create Environments on the Fly

Developer environments on demand using the new `--option` (`-O`) flag:

```shell-session
$ devenv --option languages.python.enable:bool true \
         --option packages:pkgs "ncdu git ripgrep" \
         shell
```

This command creates a temporary Python environment without writing any configuration files.

Ad-hoc environments are ideal for quickly testing languages or tools without committing to a full project setup:

```shell-session
$ devenv -O languages.elixir.enable:bool true shell iex
```

## Supported Option Types

The `--option` flag supports multiple data types, making it flexible for various use cases:

- `:string` for text values
- `:int` for integers
- `:float` for decimal numbers
- `:bool` for true/false values
- `:path` for file paths
- `:pkgs` for specifying Nix packages

## GitHub Actions with Matrices

One of the most powerful applications of ad-hoc environments is in CI pipelines, where you can easily implement testing matrices across different configurations:

```yaml
jobs:
  test:
    strategy:
      matrix:
        python-version: ['3.9', '3.10', '3.11']
    steps:
      - uses: actions/checkout@v3
      - uses: cachix/install-nix-action@v31
      - uses: cachix/cachix-action@v16
        with:
          name: devenv
      - name: Install devenv.sh
        run: nix profile install nixpkgs#devenv
      - name: Test with Python {{ '${{ matrix.python-version }}' }}
        run: |
          devenv --option languages.python.enable:bool true \
                 --option languages.python.version:string {{ '${{ matrix.python-version }}' }} \
                 test
```

This approach lets you validate your code across multiple language versions or dependency combinations without maintaining separate configuration files for each scenario.

## Combining with Existing Configurations

When used with an existing `devenv.nix` file, `--option` values override the configuration settings in the file, making it easy to temporarily modify your environment.

## Switching Between Environment Profiles

Ad-hoc options are perfect for switching between predefined profiles in your development environment:

```shell-session
$ devenv --option profile:string backend up
```

This enables you to switch between frontend, backend, or other custom profiles without modifying your configuration files.

See our [Profiles guide](https://devenv.sh/guides/profiles/) for more details on setting up and using profiles.

For complete documentation on this feature, visit our [Ad-hoc Developer Environments guide](https://devenv.sh/ad-hoc-developer-environments/).

We're excited to see how you'll use ad-hoc environments to streamline your development workflow. Share your feedback on [GitHub](https://github.com/cachix/devenv) or [join our Discord community](https://discord.gg/MycroftAI)!
