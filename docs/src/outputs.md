# Outputs

!!! tip "New in version 1.1"
    
    [Read more about outputs in the v1.1 release post](blog/posts/devenv-v1.1-outputs.md)

Outputs allow you to define Nix derivations using the module system,
exposing Nix packages or sets of packages to be consumed by other tools for installation/distribution.

devenv provides a unified interface for packaging applications across all supported languages,
using each language's best packaging tools automatically.


## Defining outputs

You can define outputs in your `devenv.nix` file using the `outputs` attribute.

## Language integration

Each language provides an `import` function that uses the best packaging tools for that ecosystem:

```nix
{ config, ... }: {
  languages.rust.enable = true;
  languages.python.enable = true;

  outputs = {
    rust-app = config.languages.rust.import ./rust-app {};
    python-app = config.languages.python.import ./python-app {};
  };
}
```

The language `import` functions automatically:

- **Rust**: Uses `crate2nix` for optimal Cargo.toml and Cargo.lock handling
- **Python**: Uses `uv2nix` for modern Python packaging with pyproject.toml support
- **Other languages**: Each uses the most appropriate packaging tool for that ecosystem

## Building outputs

To build all defined outputs, run:

```shell-session
$ devenv build
/nix/store/abc123def456ghi789jkl012mno345pq-rust-app-1.0
/nix/store/xyz987wvu654tsr321qpo987mnl654ki-python-app-1.0
```

This command will build all outputs and display their paths in the Nix store.

To build specific output(s), you can specify them explicitly:

```shell-session
$ devenv build outputs.rust-app
/nix/store/abc123def456ghi789jkl012mno345pq-rust-app-1.0
```

This will build only the `rust-app` output, making it easy to consume for installation or distribution.

## Defining outputs as custom module options

You can also define outputs using the module system's options.
This approach allows for more flexibility and integration with other parts of your configuration.

Here's an example:

```nix
{ pkgs, lib, config, ... }: {
  options = {
    myapp.package = pkgs.lib.mkOption {
      type = config.lib.types.outputOf lib.types.package;
      description = "The package for myapp";
      default = import ./myapp { inherit pkgs; };
      defaultText = "myapp";
    };
  };

  config = {
    outputs.git = pkgs.git;
  }
}
```

In this case, `myapp.package` is defined as an output option. When building, devenv will automatically include this output along with any others defined in the `outputs` attribute.

If you don't want to specify the output option type, you can use `config.lib.types.output` instead.
