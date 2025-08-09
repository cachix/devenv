# Using devenv with Nix Flakes

[Nix Flakes](https://wiki.nixos.org/wiki/Flakes) provide a standardized way to manage Nix projects. They allow you to:

* Specify dependencies as inputs
* Pin those dependencies in a lock file
* Define structured outputs for your project

!!! note "For those new to devenv and Nix"
    If you're new to both devenv and Nix, starting with the standard devenv CLI approach will provide the smoothest experience. [Getting started with devenv.](../getting-started.md)

You can integrate the devenv module system (languages, processes, services, etc.) into a Nix Flake as a `devShell` output. This allows devenv to work within your existing Flake-based projects.

While Flakes are more widely supported by existing tooling,  be aware that using devenv through Flakes has some performance limitations and reduced features compared to the dedicated devenv CLI, which we'll explain in the comparison below.

## Choosing between devenv and Nix Flakes

For most projects, we recommend using devenv.nix with the dedicated devenv CLI for the best developer experience. This approach offers several advantages:

* **Simplicity**: A more straightforward interface with less boilerplate
* **Performance**: Faster evaluation and more efficient caching of environments
* **Developer-focused**: Purpose-built for development environments with integrated tooling

[Getting started with devenv.](../getting-started.md)

Consider using the Flake integration when:

* You maintain an existing flake-based project ecosystem
* Your developer environment needs to be consumed by downstream flakes
* You're an experienced Nix user
* You understand and can work around the technical limitations of Flakes (evaluation model, impurity constraints, etc.)

### Comparison of features

| Feature | devenv CLI | Nix Flakes |
| ------- | ------ | ------ |
| External flake inputs | :material-check: | :material-check: |
| Shared remote configs | :material-check: | :material-check: |
| Designed for developer environments | :material-check: | :material-close: |
| Built-in container support | :material-check: | :material-close: |
| Protection from garbage-collection | :material-check: | :material-close: |
| Faster evaluation (lazy trees) | :material-check: | :material-close: |
| Evaluation caching | :material-check: | :material-close: |
| Pure evaluation | :material-check: | :material-close: (`impure` by default) |
| Export as a flake | :material-close: | :material-check: |

## Getting started

Set up a new project with Nix flakes using our template:

```console
nix flake init --template github:cachix/devenv
```

This template will create:

* A `flake.nix` file containing a basic devenv configuration.
* A `.envrc` file to optionally set up automatic shell activation with direnv.

## Working with flake shells

### The `flake.nix` file

Here's a minimal `flake.nix` to start you off that includes a `devShell` created with `devenv.lib.mkShell`.
See [the reference documentation](../reference/options.md) for the possible options to use here.

```nix
{
  inputs = {
    nixpkgs.url = "github:cachix/devenv-nixpkgs/rolling";
    devenv.url = "github:cachix/devenv";
  };

  nixConfig = {
    extra-trusted-public-keys = "devenv.cachix.org-1:w1cLUi8dv3hnoSPGAuibQv+f9TZLr6cv/Hm9XgU50cw=";
    extra-substituters = "https://devenv.cachix.org";
  };

  outputs = { self, nixpkgs, devenv, ... } @ inputs:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
    in
    {
      devShells.${system}.default = devenv.lib.mkShell {
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

### Entering the shell

Create and enter the `devenv` shell with:

```console
nix develop --no-pure-eval
```

This will evaluate the inputs to your flake, create a `flake.lock` lock file, and open a new shell using the `devenv` configuration from your `flake.nix`.

!!! note "Why do I need to use the `--no-pure-eval` flag?"
    Flakes use "pure evaluation" by default, which prevents devenv from figuring out the environment its running in: for example, querying the working directory.
    The `--no-pure-eval` flag relaxes this restriction.

    An alternative, and less flexible, workaround is to override the `devenv.root` option to the absolute path to your project directory.
    This makes the flake non-portable between machines, but does allow the shell to be evaluated in pure mode.


### Launching processes, services, and tests

Once in the shell, you can launch [processes and services with `devenv up`](../processes.md).

```console
$ devenv up
17:34:37 system | run.1 started (pid=1046939)
17:34:37 run.1  | Hello, world!
17:34:37 system | run.1 stopped (rc=0)
```

And run [tests with `devenv test`](../tests.md).

```console
$ devenv test
Running tasks     devenv:enterShell
Succeeded         devenv:git-hooks:install 10ms
Succeeded         devenv:enterShell         4ms
2 Succeeded                                 14.75ms
• Testing ...
Running tasks     devenv:enterTest
Succeeded         devenv:git-hooks:run     474ms
No command        devenv:enterTest
1 Skipped, 1 Succeeded                      474.62ms
```


### Automated shell switching

You can configure your shell to launch automatically when you enter the project directory.

First, install [nix-direnv](https://github.com/nix-community/nix-direnv).

The add the following line to your `.envrc`:

```text
use flake . --no-pure-eval
```

Allow `direnv` to evaluate the updated `.envrc`:

```console
direnv allow
```


## Multiple shells

Some projects lend themselves to defining multiple development shells. For instance, you may want to define multiple development shells for different subprojects in a monorepo.
You can do this by defining the various development shells in a central `flake.nix` file in the root of the repository.

The `flake.nix` file contains multiple `devShells`. For example:

```nix
{
  inputs = {
    nixpkgs.url = "github:cachix/devenv-nixpkgs/rolling";
    devenv.url = "github:cachix/devenv";
  };

  outputs = { self, nixpkgs, devenv, ... } @ inputs:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
    in
    {
      devShells.${system} = {
        projectA = devenv.lib.mkShell {
          inherit inputs pkgs;
          modules = [
            {
              enterShell = ''
                echo this is project A
              '';
            }
          ];
        };

        projectB = devenv.lib.mkShell {
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
    };
}
```

Here we've define two shells, each with a separate `devenv` configuration.

To enter the shell of `project A`:

```console
$ nix develop --no-pure-eval .#projectA
this is project A
(devenv) $
```

To enter the shell of `project B`:

```console
$ nix develop --no-pure-eval .#projectB
this is project B
(devenv) $
```

## External flakes

If you cannot, or don't want to, add a `flake.nix` file to your project's repository, you can use external flakes instead.

Create a separate repository with a `flake.nix` file, as in the example above. Then refer to this flake in your project:

```console
$ nix develop --no-pure-eval file:/path/to/central/flake#projectA
this is project A
(devenv) $
```

You can also add this to the `direnv` configuration of the project. Make sure the following line is in `.envrc`:

```text
nix flake --no-pure-eval file:/path/to/central/flake#projectA
```

External flakes aren't limited to local paths using `file:`. You can refer to flakes on `github:` and generic `git:` repositories.
See [Nix flake references](https://nixos.org/manual/nix/stable/command-ref/new-cli/nix3-flake.html#flake-references) for more options.

When using this method to refer to external flakes, it's important to remember that there is no lock file, so there is no certainty about which version of the flake is used.
A local project flake file will give you more control over which version of the flake is used.
