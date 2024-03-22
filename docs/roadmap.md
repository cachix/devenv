# Roadmap

## Running devenv in a container

While devenv is designed to be run on your local machine, we are looking to add support for [running devenv inside a container](https://github.com/cachix/devenv/issues/1010).

Something like:

```
devenv shell --in-container
devenv test --in-container
```

This would be convenient when the environment is too complex to set up on your local machine; for example, when running two databases or when you want to run tests in a clean environment.

## Generating containers with full environment

Currently, `enterShell` is executed only once the container has started.
If we want to execute it as part of the container generation, we have
to [execute it inside a container to generate a layer](https://github.com/cachix/devenv/issues/997).

## macOS support for generating containers

Building containers on macOS is not currently supported,
but it [should be possible](https://github.com/cachix/devenv/issues/997).

## Native mapping of dependencies

Wouldn't it be cool if devenv could map language-specific dependencies to your local system? In this example, devenv should be able to determine that `pillow` requires `pkgs.cairo`:

```nix
{ pkgs, lib, ... }: {
  languages.python = {
    enable = true;
    venv.enable = true;
    venv.requirements = ''
      pillow
    '';
  };
}
```