# [devenv.sh](https://devenv.sh) - Fast, Declarative, Reproducible, and Composable Developer Environments

[![Join Discord](https://img.shields.io/discord/1036369714731036712?color=7389D8&label=discord&logo=discord&logoColor=ffffff)](https://discord.gg/naMgvexb6q) 
![License: Apache 2.0](https://img.shields.io/github/license/cachix/devenv) 
[![version](https://img.shields.io/github/v/release/cachix/devenv?color=green&label=version&sort=semver)](https://github.com/cachix/devenv/releases) 
[![CI](https://github.com/cachix/devenv/actions/workflows/buildtest.yml/badge.svg)](https://github.com/cachix/devenv/actions/workflows/buildtest.yml?branch=main)

![logo](docs/assets/logo.webp)

Running ``devenv init`` generates ``devenv.nix``:

```nix
{ pkgs, ... }:

{
  # https://devenv.sh/basics/
  env.GREET = "devenv";

  # https://devenv.sh/packages/
  packages = [ pkgs.git ];

  enterShell = ''
    hello
    git --version
  '';

  # https://devenv.sh/languages/
  languages.nix.enable = true;

  # https://devenv.sh/scripts/
  scripts.hello.exec = "echo hello from $GREET";

  # https://devenv.sh/pre-commit-hooks/
  pre-commit.hooks.shellcheck.enable = true;

  # https://devenv.sh/processes/
  # processes.ping.exec = "ping example.com";
}

```

And ``devenv shell`` activates the environment.

## Commands

- ``devenv init``:           Scaffold devenv.yaml, devenv.nix, and .envrc.
- ``devenv shell``:          Activate the developer environment.
- ``devenv shell CMD ARGS``: Run CMD with ARGS in the developer environment.
- ``devenv update``:         Update devenv.lock from devenv.yaml inputs. See http://devenv.sh/inputs/#locking-and-updating-inputs.
- ``devenv up``:             Start processes in foreground. See http://devenv.sh/processes.
- ``devenv gc``:             Remove old devenv generations. See http://devenv.sh/garbage-collection.
- ``devenv ci``:             Build your developer environment and make sure all checks pass.

## Documentation

- [Getting Started](https://devenv.sh/getting-started/)
- [Basics](https://devenv.sh/basics/)
- [Roadmap](https://devenv.sh/roadmap/)
- [Blog](https://devenv.sh/blog/)
- [`devenv.yaml` reference](https://devenv.sh/reference/yaml-options/)
- [`devenv.nix` reference](https://devenv.sh/reference/options/)
- [Contributing](https://devenv.sh/community/contributing/)
