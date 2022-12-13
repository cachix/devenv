`devenv` can be used without the `devenv` CLI by integrating into [Nix Flakes](https://www.tweag.io/blog/2020-05-25-flakes/), if you're more familiar with the Nix language/ecosystem.

Some usecases for using devenv configuration inside flakes is for projects that want to define other Nix flake features, apart from the development shell.
These include a Nix package for the project, NixOS and home-manager modules related to the project.
Usually you want to use the same lock file for the development shell as well as the Nix package and others, so that everything is based on the same nixpkgs.

A Nix flake includes the inputs from `devenv.yaml` as well as the devenv configuration that you'd usually find in `devenv.nix`. `flake.lock` is the lock file for Nix flakes, the equivalent to `devenv.lock`.

## Getting started

To quickly set a project up with Nix flakes, use of `nix flake init`, like:

```console
$ nix flake init --template github:cachix/devenv
```

This will create a `flake.nix` with devenv configuration, as well as a `.envrc` direnv configuration.

Open the devenv shell using:

```console
$ nix develop
```

This will create a lock file and open up a new shell that adheres to the devenv configuration stated in `flake.nix`.

## flake.nix

A minimal flake.nix that includes devenv is for example:

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
    {
      devShell.x86_64-linux = devenv.lib.mkShell {
        inherit inputs pkgs;
        modules = [
          {
            # This is where devenv configuration can be stated
            packages = [ pkgs.hello ];

            enterShell = ''
              hello
            '';
          }
        ];
      };
    };
}
```

Here a single shell is defined. It is defined _only_ for a `x86_64-linux` system. The shell includes a single devenv configuration module.
Inside the module is where you put the devenv configuration, the one you usually will find in `devenv.nix`. See https://devenv.sh/reference/options/ for the possible options to use here.

## direnv

To make use of `direnv` in your Nix flake project, you'll need [nix-direnv](https://github.com/nix-community/nix-direnv).

To configure `direnv` in your project make sure you have a file called `.envrc` that includes the following line:

```text
nix flake --impure
```

In normal `nix flake` projects, `--impure` is not needed. When using `devenv` in your flake, you _do_ need this option.

## Multiple shells

Defining multiple development shells using flakes can be useful depending on your projects structure. We will handle 2 use-cases here.

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
