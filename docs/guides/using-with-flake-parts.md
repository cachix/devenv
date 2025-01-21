If you're familiar with the Nix language and ecosystem, `devenv` can be used without the `devenv` CLI by integrating into [Nix Flakes](https://www.tweag.io/blog/2020-05-25-flakes/) using [flake-parts](https://flake.parts).

Using a `devenv` configuration in flakes is useful for projects that need to define other Nix flake features in addition to the development shell.
Additional flake features may include the Nix package for the project, or NixOS and Home Manager modules related to the project.
Using the same lock file for the development shell and other features ensures that everything is based on the same `nixpkgs`.

A Nix flake needs to consist of at least the input declarations from `devenv.yaml`, as well as the `devenv` configuration that you would usually find in `devenv.nix`.
`flake.lock` is the lock file for Nix flakes, the equivalent of `devenv.lock`.

## Getting started

To quickly set up project with Nix flakes, use `nix flake init`:

```console
nix flake init --template github:cachix/devenv#flake-parts
```

This will create a `flake.nix` file with a basic `devenv` configuration and a `.envrc` file for direnv support.

## Working with flake shells

### The `flake.nix` file

Here's an example of a minimal `flake.nix` file that includes `devenv`:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.05";
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

Here a single shell is defined for all listed [systems](https://flake.parts/options/flake-parts.html#opt-systems).
The shell includes a single `devenv` configuration module, under [`devenv.shells`](https://flake.parts/options/devenv.html#opt-perSystem.devenv.shells), named `default`.

Add your `devenv` configuration (usually in the `devenv.nix` file) to this module.
See [`devenv.nix` options](../reference/options.md) for more information about configuration options.


### Entering the shell

Enter the `devenv` shell using:

```console
nix develop --no-pure-eval
```

This will create a lock file and open a new shell using the `devenv` configuration from your `flake.nix`.

!!! note "Why do I need to use the `--no-pure-eval` flag?"
    Flakes use "pure evaluation" by default, which prevents devenv from figuring out the environment its running in: for example, querying the working directory.
    The `--no-pure-eval` flag relaxes this restriction.

    An alternative, and less flexible, workaround is to override the `devenv.root` option to the absolute path to your project directory.
    This makes the flake non-portable between machines, but does allow the shell to be evaluated in pure mode.


### Launching processes, services, and tests

Once in the shell, you can launch [processes and services with `devenv up`](/processes).

```console
$ devenv up
17:34:37 system | run.1 started (pid=1046939)
17:34:37 run.1  | Hello, world!
17:34:37 system | run.1 stopped (rc=0)
```

And run [tests with `devenv test`](/tests).

```console
$ devenv test
Running tasks     devenv:enterShell
Succeeded         devenv:git-hooks:install 10ms
Succeeded         devenv:enterShell         4ms
2 Succeeded                                 14.75ms
• Testing ...
Running tasks     devenv:enterTest
Succeeded         devenv:git-hooks:run     474ms
Not implemented   devenv:enterTest
1 Skipped, 1 Succeeded                      474.62ms
```

### Import a devenv module

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

### Automated shell switching

You can configure your shell to launch automatically when you enter the project directory.

First, install [nix-direnv](https://github.com/nix-community/nix-direnv).

Then add the following line to your `.envrc`:

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

The `flake.nix` file outputs multiple [`devShells`](https://flake.parts/options/flake-parts.html#opt-flake.devShells) when you provide multiple [perSystem.devenv.shells](https://flake.parts/options/devenv.html#opt-perSystem.devenv.shells) definitions.
For example:

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
    echo this is project B
    hello
  '';
};

# If you'd like to pick a default
devShells.default = config.devShells.projectA;
```

Here we have defined two shells, each with a `devenv` configuration and differently defined [`enterShell`](../reference/options.md#entershell) command.

To enter the shell of `projectA`:

```console
$ nix develop --no-pure-eval .#projectA
this is project A
(devenv) $ 
```

To enter the shell of `projectB`:

```console
$ nix develop --no-pure-eval .#projectB
this is project B
(devenv) $ 
```

The last line makes `projectA` the default shell:

```console
$ nix develop --no-pure-eval .
this is project A
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
