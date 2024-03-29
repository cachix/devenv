If you're familiar with the Nix language and ecosystem, `devenv` can be used without the `devenv` CLI by integrating into [Nix Flakes](https://www.tweag.io/blog/2020-05-25-flakes/) using [flake-parts](https://flake.parts).

Using `devenv` configuration in flakes is useful for projects that need to define other Nix flake features in addition to the development shell.
Additional flake features may include the Nix package for the project or NixOS and Home Manager modules related to the project.
Using the same lock file for the development shell and other features ensures that everything is based on the same `nixpkgs`.

A Nix flake needs to consist of at least the input declarations from `devenv.yaml`, as well as the `devenv` configuration that you would usually find in `devenv.nix`. `flake.lock` is the lock file for Nix flakes, the equivalent to `devenv.lock`.

## Getting started

To quickly set a project up with Nix flakes, use `nix flake init`:

```console
$ nix flake init --template github:cachix/devenv#flake-parts
```

This will create a `flake.nix` file with `devenv` configuration and a `.envrc` file with direnv configuration.

Open the `devenv` shell using:

```console
$ nix develop --impure
```

This will also create a lock file and open a new shell that adheres to the `devenv` configuration contained in `flake.nix`.

## The `flake.nix` file

Here's an example of a minimal `flake.nix` file that includes `devenv`:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-23.05";
    devenv.url = "github:cachix/devenv";
  };

  outputs = inputs@{ flake-parts, nixpkgs, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [
        inputs.devenv.flakeModule
      ];
      systems = nixpkgs.lib.systems.flakeExposed;

      perSystem = { config, self', inputs', pkgs, system, ... }: {
        # Per-system attributes can be defined here. The self' and inputs'
        # module parameters provide easy access to attributes of the same
        # system.

        # Equivalent to  inputs'.nixpkgs.legacyPackages.hello;
        packages.default = pkgs.hello;

        devenv.shells.default = {
          # https://devenv.sh/reference/options/
          packages = [ config.packages.default ];

          enterShell = ''
            hello
          '';
        };
      };
    };
}
```

Here a single shell is defined for all listed [systems](https://flake.parts/options/flake-parts.html#opt-systems). The shell includes a single `devenv` configuration module, under [`devenv.shells`](https://flake.parts/options/devenv.html#opt-perSystem.devenv.shells), named `default`.

Add your `devenv` configuration (usually in the `devenv.nix` file) to this module. See [`devenv.nix` options](../reference/options.md) for more information about configuration options.

## The direnv extension

To use direnv in your Nix flake project, you'll need [nix-direnv](https://github.com/nix-community/nix-direnv).

To configure direnv, ensure your project has a `.envrc` file that includes the following line:

```text
use flake . --impure
```

In a standard Nix flake project, the `--impure` flag is not needed. However, using `devenv` in your flake _requires_ the `--impure` flag.

## Import a devenv module

You can import a devenv configuration or module, such as `devenv-foo.nix` into an individual shell as follows.

Add `imports` to your `devenv.shells.<name>` definition:

```nix
# inside perSystem = { ... }: {

devenv.shells.default = {
  imports = [ ./devenv-foo.nix ];

  enterShell = ''
    hello
  '';
};
```

You can use definitions from your flake in your devenv configuration.
When you do so it's recommended to use a different file name than `devenv.nix`, because it may not be standalone capable.

For example, if `devenv-foo.nix` declares a devenv [service](../services.md), and you've packaged it locally into [`perSystem.packages`](https://flake.parts/options/flake-parts.html#opt-perSystem.packages), you can provide the package as follows:

```nix
# inside perSystem = { config, ... }: {

devenv.shells.default = {
  imports = [ ./devenv-foo.nix ];

  services.foo.package = config.packages.foo;

  enterShell = ''
    hello
  '';
};
```

Your devenv module then doesn't have to provide a default:

```nix
{ config, lib, ... }:
let cfg = config.services.foo;
in {
  options = {
    services.foo = {
      package = lib.mkOption {
        type = lib.types.package;
        defaultText = lib.literalMD "defined internally";
        description = "The foo package to use.";
      };
      # ...
    };
  };
  config = lib.mkIf cfg.enable {
    processes.foo.exec = "${cfg.package}/bin/foo";
  };
}
```

## Multiple shells

Depending on the structure of your project, you may want to define multiple development shells using flakes. We'll take a look at two use cases for multiple shells here: A single project with multiple shells and a project with an external flake.

### Single project with multiple shells

Some projects lend themselves to defining multiple development shells. For instance, you may want to define multiple development shells for different subprojects in a monorepo. You can do this by defining the various development shells in a central `flake.nix` file in the root of the repository.

The `flake.nix` file outputs multiple [`devShells`](https://flake.parts/options/flake-parts.html#opt-flake.devShells) when you provide multiple [perSystem.devenv.shells](https://flake.parts/options/devenv.html#opt-perSystem.devenv.shells) definitions. For example:

```nix
# inside perSystem = { ... }: {

devenv.shells.projectA = {
  # https://devenv.sh/reference/options/
  packages = [ config.packages.default ];

  enterShell = ''
    echo this is project A
    hello
  '';
};

devenv.shells.projectB = {
  # https://devenv.sh/reference/options/
  packages = [ config.packages.default ];

  enterShell = ''
    echo this is project A
    hello
  '';
};

# If you'd like to pick a default
devShells.default = config.devShells.projectA;
```

Here we have defined two shells, each with a `devenv` configuration and differently defined [`enterShell`](../reference/options.md#entershell) command.

To enter the shell of `projectA`:

```console
$ nix develop --impure .#projectA
this is project A
(devenv) $ 
```

To enter the shell of `projectB`:

```console
$ nix develop --impure .#projectB
this is project B
(devenv) $ 
```

The last line makes `projectA` the default shell:

```console
$ nix develop --impure .
this is project A
(devenv) $ 
```

### Projects with an external flake

If you cannot or don't want to add a `flake.nix` file to your project repository, you can refer to external flakes.

You can create a repository with a `flake.nix` file as in the example above. You can now refer to this flake in a different project:

```console
$ nix develop --impure file:/path/to/central/flake#projectA
this is project A
(devenv) $ 
```

You can also add this to the direnv configuration of the project. Just make sure the following line is in `.envrc`:

```text
nix flake --impure file:/path/to/central/flake#projectA
```

Note that instead of referring to a directory on a local file system that includes the `flake.nix`, like `/path/to/central/flake`, it is also possible to use different references to a flake, for instance, `github:` or `git:`. See [Nix flake references](https://nixos.org/manual/nix/stable/command-ref/new-cli/nix3-flake.html#flake-references) for more information.

When using this method to refer to external flakes, it's important to remember that there is no lock file, so there is no certainty about which version of the flake is used. A local project flake file will give you more control over which version of the flake is used.
