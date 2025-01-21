---
draft: false
date: 2024-09-11
authors:
  - domenkozar
---

# devenv 1.1: Nested Nix outputs using the module system

[devenv 1.1](https://github.com/cachix/devenv/releases/tag/v1.1) brings support for Nix outputs, matching the last missing piece of functionality with Flakes.
<br><br>

It was designed to make outputs extensible, nested, and [buildable as a whole by default](https://github.com/NixOS/nix/issues/7165).
<br><br>

This allows exposing Nix packages for installation/consumption by other tools.


## Nested Nix outputs

If you have a devenv with outputs like this:

```nix title="devenv.nix"
{ pkgs, ... }: {
  outputs = {
    myproject.myapp = import ./myapp { inherit pkgs; };
    git = pkgs.git;
  };
}
```

You can build all outputs by running:

```shell-session
$ devenv build
/nix/store/mzq5bpi49h26cy2mfj5a2r0q69fh3a9k-git-2.44.0
/nix/store/mzq5bpi49h26cy2mfj5a2r0q71fh3a9k-myapp-1.0
```

Or build specific attribute(s) by listing them explicitly:

```shell-session
$ devenv build outputs.git
/nix/store/mzq5bpi49h26cy2mfj5a2r0q69fh3a9k-git-2.44.0
```

This is useful for tools that need to find and install specific outputs.

## Defining outputs as module options

By default, any derivation specified in `outputs` nested attributes set is recognized as an output.
<br><br>

You can define custom options as output types in `devenv`. These will be automatically detected and built:

```nix title="devenv.nix"
{ pkgs, lib, config, ... }: {
  options = {
    myapp.package = lib.mkOption {
        type = config.lib.types.outputOf lib.types.package;
        description = "The package for myapp";
        default = import ./myapp { inherit pkgs; };
        defaultText = "myapp-1.0";
    };
  };

  config = {
    outputs.git = pkgs.git;
  }
}
```

Building will pick up all outputs, in this case `myapp.package` and `outputs.git`:

```shell-session
$ devenv build
/nix/store/mzq5bpi49h26cy2mfj5a2r0q69fh3a9k-myapp-1.0
/nix/store/mzq5bpi49h26cy2mfj5a2r0q69fh3a9k-git-2.44.0
```

If you don't want to specify the output type, you can just use `config.lib.types.output`.


## Referencing outputs from another devenv

If you [import another `devenv.nix` file](/composing-using-imports), the outputs will be merged together,
allowing you to compose a developer environment and outputs in one logical unit.
<br><br>

You could also import outputs from other applications as inputs instead of composing them.

[Leave a thumbs on the issue](https://github.com/cachix/devenv/issues/1438) if you'd like to see it happen.

## Documentation

See [Outputs](/outputs) section in documentation for the latest comprehensive guide to outputs.
<br><br>

We're on [Discord](https://discord.gg/naMgvexb6q) if you need help, Domen
