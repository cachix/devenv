`devenv` can be integrated with [Nix Flakes](https://www.tweag.io/blog/2020-05-25-flakes/) if you're more familiar with the Nix language and ecosystem.

You can define your own packages, NixOS and home-manager modules, but still benefit from devenv's development features.
The development shell will share the same inputs and lock file as all the other outputs of your flake, ensuring that your entire project is using the same `nixpkgs` revision.

With flakes, you also no longer need dedicated configuration files for devenv:

* the inputs from `devenv.yaml` are replaced by the flake's inputs
* `devenv.nix` becomes a shell module in `flake.nix`
* the `devenv.lock` is replaced by the `flake.lock`

## Getting started

Set up a new project with Nix flakes using our template:

```console
$ nix flake init --template github:cachix/devenv
```

This template will create:

* a `flake.nix` containing a basic devenv configuration
* an `.envrc` to optionally set up automatic shell activation with direnv.

Open the devenv shell with:

```console
$ nix develop --impure
```

This will create a `flake.lock` lock file and open up a new shell based on the devenv configuration stated in `flake.nix`.

!!! note "Why do I need `--impure`?"
    When working with flakes, pure mode prevents devenv from accessing and modifying its state data.
    Certain features, like running processes with `devenv up`, won't work in pure mode.

## Modifying your `flake.nix`

Here's a minimal `flake.nix` that includes:

* a single `devShell` for the `x86_64-linux` system created with `devenv.lib.mkShell`.
* a shell module containing the devenv configuration  — what you'd usually write in `devenv.nix`.
  See [the reference documentation](/reference/options/) for the possible options to use here.

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-22.11";
    devenv.url = "github:cachix/devenv";
  };

  outputs = { self, nixpkgs, devenv, ... } @ inputs:
    let
      pkgs = import nixpkgs {
        system = "x86_64-linux";
      };
    in
    {
      devShell.x86_64-linux = devenv.lib.mkShell {
        inherit inputs pkgs;
        modules = [
          ({ pkgs, ... }: {
            # This is your devenv configuration
            packages = [ pkgs.hello ];

            enterShell = ''
              hello
            '';

            processes.run.exec = hello;
          })
        ];
      };
    };
}
```

Once inside the shell, you can launch [process and services with `devenv up`](/processes).

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
nix flake --impure
```

## Multiple shells

Defining multiple development shells using flakes can be useful depending on your project's structure. We will handle 2 use-cases here.

### Single project with multiple shells

Some projects lend themselves to define multiple development shells. For instance, a mono-repo where you want to define multiple development shells in a central flake.nix in the root of the repository. There you can centrally define the development shells for the different sub-projects in the repository.

In this case you want to define a `flake.nix` file that contains multiple `devShells`. For example:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-22.11";
    devenv.url = "github:cachix/devenv";
  };

  outputs = { self, nixpkgs, devenv, ... } @ inputs:
    let
      pkgs = import nixpkgs {
        system = "x86_64-linux";
      };
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

Here you can see that there are 2 shells defined. Each one with a devenv configuration with differently defined `enterShell`.

To enter the shell of `projectA`:

```console
$ nix develop .#projectA
this is project A
(devenv) $ 
```

To enter the shell of `projectB`:

```console
$ nix develop .#projectB
this is project B
(devenv) $ 
```

### Projects with an external flake

Whenever you have projects where you cannot (or don't want to) add a flake.nix to its repository, you can refer to external flakes.

You can create a repository with a flake.nix like the one above. However, in a different project you can now refer to this flake using:

```console
$ nix develop file:/path/to/central/flake#projectA
this is project A
(devenv) $ 
```

You can also add this to the `direnv` configuration of the project. Just make sure the following line is in `.envrc`:

```text
nix flake --impure file:/path/to/central/flake#projectA
```

Note that instead of referring to a directory on local file system that includes the `flake.nix`, like `/path/to/central/flake`, it is also possible to use different references to a flake. For instance `github:` or `git:`. See [Nix flake references](https://nixos.org/manual/nix/stable/command-ref/new-cli/nix3-flake.html#flake-references) for more information.

One big caveat with this method is that there is no lock file. It is not 100% clear which version of the flake is used when referring to it this way. A local project flake file will give more control which version of the flake is used.
