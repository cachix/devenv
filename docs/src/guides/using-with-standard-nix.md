# Using devenv with standard Nix

[Nix Flakes](https://wiki.nixos.org/wiki/Flakes) are not standard nix, and for those who prefer not to,
or can't use them but still want to pin and share dependencies across multiple targets, there is a way.
Without flakes or the devenv CLI, we can download inputs using built-in functions like fetchGit or fetchTarball,
or using popular dependency pinning tools like [niv](https://github.com/nmattia/niv) or [npins](https://github.com/andir/npins).
Then we can define devenv shells as output attributes like anything else in Nix.

!!! note "For those new to devenv and Nix"
    If you're new to both devenv and Nix, starting with the standard devenv CLI approach will provide the smoothest experience. [Getting started with devenv.](../getting-started.md)

You can integrate the devenv module system (languages, processes, services, etc.) into a Nix module as an attribute.
This allows using devenv without flakes or the devenv CLI and replace standard nix shells in a painless way within
your existing nix-based development environments.

Creating a devenv shell via the non-flake wrapper is not a first-class option and might have some limitation compared to the other methods.
It is only recommended to experienced Nix users who prefer to not use flakes.


## Getting started

In this guide we will show how to write a basic `default.nix` using the built-in
fetchTarball to fetch sources from GitHub and a devenv shell definition.

Create a default.nix file in the project root with the following content:

```nix title="default.nix"
let
  nixpkgs = fetchTarball "https://github.com/cachix/devenv-nixpkgs/archive/rolling.tar.gz";
  pkgs = import nixpkgs { };
  devenv-src = fetchTarball "https://github.com/cachix/devenv/archive/main.tar.gz";
  devenv = (import devenv-src).lib.mkStandardShell;
in
{
  shell = devenv {
    inherit pkgs;
    modules = [ ./devenv.nix ];
  };
}
```

Finally, create a standard devenv configuration in `devenv.nix` [as normal](https://devenv.sh/basics/).


### Entering the shell

Create and enter the `devenv` shell with:

```console
nix-shell
```


### Further considerations

The created `default.nix` file is standard Nix.
You can freely define multiple attributes, such as more shells, packages, NixOSand Home Manager configurations,
among other things, as well as add more input sources as necessary.

The non-flake warpper is a thin wrapper created around the flake based shell function, which was written with flakes in mind.
In some cases, such as Rust channel configurations, devenv requires extra inputs like the rust-overlay flake.
If that input is not provided, devenv will print error messages with some instructions meant for a flake-based setup.
This scenario can be handled the following way with a non-flake setup:

```nix title="default.nix"
let
  nixpkgs = fetchTarball "https://github.com/cachix/devenv-nixpkgs/archive/rolling.tar.gz";
  pkgs = import nixpkgs { };
  devenv-src = fetchTarball "https://github.com/cachix/devenv/archive/main.tar.gz";
  devenv = (import devenv-src).lib.mkStandardShell;
  flake-compat = import (fetchTarball "https://github.com/NixOS/flake-compat/archive/master.tar.gz");
  rust-overlay-src = fetchTarball "https://github.com/oxalica/rust-overlay/archive/master.tar.gz";
in
{
  shell = devenv {
    inherit pkgs;
    modules = [ ./devenv.nix ];
    inputs = {
      nixpkgs = pkgs;
      rust-overlay = (flake-compat { src = rust-overlay-src; }).defaultNix;
    };
  };
}
```

The reason we need to use flake-compat here instead of directly importing the source,
is that rust-overlay has a `default.nix` file that is structured differently from their flake,
and devenv expects an attribute set that is structured like the flake.
There might be other scenarios that need to be handled differently.
