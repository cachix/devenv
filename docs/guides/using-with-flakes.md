If you're familiar with the Nix language and ecosystem, `devenv` can be integrated with [Nix Flakes](https://www.tweag.io/blog/2020-05-25-flakes/).

You can define your own packages, NixOS and Home Manager modules, and benefit from the `devenv` development features.
The development shell will share the same inputs and lock file as all the other outputs of your flake, ensuring that your entire project is using the same `nixpkgs` revision.

With flakes, you no longer need dedicated configuration files for `devenv`:

* The inputs from `devenv.yaml` are replaced by the flake's inputs.
* `devenv.nix` becomes a shell module in `flake.nix`.
* The `devenv.lock` is replaced by the `flake.lock`.

## Getting started

Set up a new project with Nix flakes using our template:

```console
$ nix flake init --template github:cachix/devenv
```

This template will create:

* A `flake.nix` file containing a basic devenv configuration.
* A `.envrc` file to optionally set up automatic shell activation with direnv.

Open the `devenv` shell with:

```console
$ nix develop --impure
```

This will create a `flake.lock` lock file and open a new shell based on the `devenv` configuration stated in `flake.nix`.

!!! note "Why do I need `--impure`?"
    When working with flakes, pure mode prevents `devenv` from accessing and modifying its state data.
    Certain features, like running processes with `devenv up`, won't work in pure mode.

## Modifying your `flake.nix` file

Here's a minimal `flake.nix` file that includes:

* A single `devShell` for the `x86_64-linux` system created with `devenv.lib.mkShell`.
* A shell module containing the `devenv` configuration. This is what you'd usually write in `devenv.nix`.
  See [the reference documentation](/reference/options/) for the possible options to use here.

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-23.05";
    devenv.url = "github:cachix/devenv";
  };

  nixConfig = {
    extra-trusted-public-keys = "devenv.cachix.org-1:w1cLUi8dv3hnoSPGAuibQv+f9TZLr6cv/Hm9XgU50cw=";
    extra-substituters = "https://devenv.cachix.org";
  };

  outputs = { self, nixpkgs, devenv, ... } @ inputs:
    let
      pkgs = nixpkgs.legacyPackages."x86_64-linux";
    in
    {
      devShell.x86_64-linux = devenv.lib.mkShell {
        inherit inputs pkgs;
        modules = [
          ({ pkgs, config, ... }: {
            # This is your devenv configuration
            packages = [ pkgs.hello ];

            enterShell = ''
              hello
            '';

            processes.run.exec = "hello";
          })
        ];
      };
    };
}
```

Once in the shell, you can launch [processes and services with `devenv up`](/processes).

```console
$ devenv up
17:34:37 system | run.1 started (pid=1046939)
17:34:37 run.1  | Hello, world!
17:34:37 system | run.1 stopped (rc=0)
```

## Automatic shell switching with direnv

Install [nix-direnv](https://github.com/nix-community/nix-direnv) for direnv to work with flakes.

Add the following line to your `.envrc`:

```console
use flake . --impure
```

## Multiple shells

Defining multiple development shells using flakes can be useful depending on your project's structure. We will handle two use cases here.

### Single project with multiple shells

Some projects lend themselves to defining multiple development shells. For instance, you may want to define multiple development shells for different subprojects in a monorepo. You can do this by defining the various development shells in a central `flake.nix` file in the root of the repository. 

The `flake.nix` file contains multiple `devShells`. For example:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-23.05";
    devenv.url = "github:cachix/devenv";
  };

  outputs = { self, nixpkgs, devenv, ... } @ inputs:
    let
      pkgs = nixpkgs.legacyPackages."x86_64-linux";
    in
    {
      devShell.x86_64-linux.projectA = devenv.lib.mkShell {
        inherit inputs pkgs;
        modules = [
          {
            enterShell = ''
              echo this is project A
            '';
          }
        ];
      };
      devShell.x86_64-linux.projectB = devenv.lib.mkShell {
        inherit inputs pkgs;
        modules = [
          {
            enterShell = ''
              echo this is project B
            '';
          }
        ];
      };
    };
}
```

Here we define two shells, each with a `devenv` configuration and differently defined `enterShell` command.

To enter the shell of `project A`:

```console
$ nix develop --impure .#projectA
this is project A
(devenv) $ 
```

To enter the shell of `project B`:

```console
$ nix develop --impure .#projectB
this is project B
(devenv) $ 
```

### Projects with an external flake

If you cannot or don't want to add a `flake.nix` file to your project's repository, you can refer to external flakes.

You can create a repository with a `flake.nix` file as in the example above. You can now refer to this flake in a different project:

```console
$ nix develop --impure file:/path/to/central/flake#projectA
this is project A
(devenv) $ 
```

You can also add this to the `direnv` configuration of the project. Make sure the following line is in `.envrc`:

```text
nix flake --impure file:/path/to/central/flake#projectA
```

Note that instead of referring to a directory on the local file system that includes the `flake.nix` file, like `/path/to/central/flake`, it is also possible to use different references to a flake. For instance `github:` or `git:`. See [Nix flake references](https://nixos.org/manual/nix/stable/command-ref/new-cli/nix3-flake.html#flake-references) for more information.

When using this method to refer to external flakes, it's important to remember that there is no lock file, so there is no certainty about which version of the flake is used. A local project flake file will give you more control over which version of the flake is used.
