---
date: 2025-04-22
title: "devenv v1.6: Ad-hoc Developer Environments"
authors:
  - domen
draft: true
---

# devenv v1.6: Ad-hoc Developer Environments

We're excited to announce the release of devenv 1.6, which introduces a powerful new feature: ad-hoc developer environments. This feature allows you to create temporary environments directly from the command line without needing a `devenv.nix` file.

## Create Environments on the Fly

With devenv 1.6, you can now create developer environments on demand using the new `--option` (`-O`) flag:

```shell-session
$ devenv --option languages.python.enable:bool true \
         --option languages.python.version:string "3.10" \
         shell
```

This creates a temporary Python 3.10 environment without writing a single line of configuration. When you're done, the environment disappears, leaving no trace on your system.

## Versatile Option Types

The `--option` flag supports multiple data types, making it flexible for various use cases:

- `:string` for text values
- `:int` for integers
- `:float` for decimal numbers
- `:bool` for true/false values
- `:path` for file paths
- `:pkgs` for specifying Nix packages

For example, to quickly create an environment with specific tools:

```shell-session
$ devenv --option packages:pkgs "ncdu git ripgrep" shell
```

## Try Languages and Tools Instantly

Ad-hoc environments are perfect for quickly testing languages or tools without committing to a project setup:

```shell-session
$ devenv --option languages.rust.enable:bool true \
         --option languages.rust.channel:string nightly \
         shell
```

Or launch directly into a language REPL:

```shell-session
$ devenv --option languages.elixir.enable:bool true shell iex
```

## Power Up CI with Environment Matrices

One of the most powerful applications of ad-hoc environments is in CI pipelines, where you can easily create testing matrices across different configurations:

```yaml
jobs:
  test:
    strategy:
      matrix:
        python-version: ['3.9', '3.10', '3.11']
    steps:
      - uses: actions/checkout@v3
      - name: Test with Python ${{ matrix.python-version }}
        run: |
          devenv --option languages.python.enable:bool true \
                 --option languages.python.version:string ${{ matrix.python-version }} \
                 test
```

This allows you to validate your code across multiple language versions or dependency combinations without maintaining separate configuration files for each scenario.

## Ideal Use Cases

This feature is particularly useful for:

1. Quick testing of tools or languages
2. One-off tasks requiring specific dependencies
3. Trying different configurations before creating a `devenv.nix` file
4. Creating lightweight environments without project setup
5. Testing across different versions of languages or tools in CI

## Combining with Existing Configurations

When used with an existing `devenv.nix` file, `--option` values will override the configuration in the file, making it easy to temporarily modify your environment.

For full documentation on this feature, visit our [Ad-hoc Developer Environments guide](https://devenv.sh/ad-hoc-developer-environments/).

We're excited to see how you'll use ad-hoc environments to streamline your development workflow. Let us know what you think on [GitHub](https://github.com/cachix/devenv) or [join our Discord community](https://discord.gg/MycroftAI)!