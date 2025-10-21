# Using devenv with standard Nix

[Nix Flakes](https://wiki.nixos.org/wiki/Flakes) are not standard nix, and for those who prefer not to,
or can't use them but still want to pin and share dependencies across multiple targets, there is a way.
Without flakes or the devenv CLI, you can pin inputs to specific versions with tools like
[npins](https://github.com/andir/npins) and define devenv shells as output attributes like anything else in Nix.

!!! note "For those new to devenv and Nix"
    If you're new to both devenv and Nix, starting with the standard devenv CLI approach will provide the smoothest experience. [Getting started with devenv.](../getting-started.md)

You can integrate the devenv module system (languages, processes, services, etc.) into a Nix module as an attribute.
This allows using devenv without flakes or the devenv CLI and replace standard nix shells in a painless way within
your existing nix-based development environments.

Creating a devenv shell via the non-flake wrapper is not a first-class option and might have some limitation compared to the other methods.
It is only recommended to experienced Nix users who prefer to not use flakes.


## Getting started

In this guide we will show how to write a basic `default.nix` with sources from npins and a devenv shell definition.
First, in your project root directory run the following commands, or follow the [getting started](https://github.com/andir/npins?tab=readme-ov-file#getting-started) guide from npins.

```console
nix-shell -p npins
npins init
npins add github cachix devenv 
```

Then create a default.nix file in the project root with the following content:

```nix title="default.nix"
let
  sources = import ./npins;
  pkgs = import sources.nixpkgs { };
  devenv = (import sources.devenv).lib.nonFlakeMkShell ./.;
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

```console
nix-shell -p npins
npins init
npins add github cachix devenv
npins add github edolstra flake-compat
npins add github oxalica rust-overlay -b master
```

```nix title="default.nix"
let
  sources = import ./npins;
  pkgs = import sources.nixpkgs { };
  devenv = (import sources.devenv).lib.nonFlakeMkShell ./.;
  flake-compat = import sources.flake-compat;
in
{
  # Shell configs
  shell = devenv {
    inherit pkgs;
    modules = [ ./nix/shell.nix ];
    inputs = {
      nixpkgs = pkgs;
      rust-overlay = (flake-compat { src = sources.rust-overlay; }).defaultNix;
    };
  };
}
```

The reason we need to use flake-compat here instead of directly importing the source,
is that rust-overlay has a `default.nix` file that is structured differently from their flake,
and devenv expects an attribute set that is structured like the flake.
There might be other scenarios that need to be handled differently.
