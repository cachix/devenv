# Outputs

!!! info "New in version 1.1"

Outputs allow you to define Nix derivations using the module system,
exposing Nix packages or sets of packages to be consumed by other tools for installation/distribution.


## Defining outputs

You can define outputs in your `devenv.nix` file using the `outputs` attribute. Here's a simple example:

```nix
{ pkgs, ... }: {
  outputs = {
    myproject.myapp = import ./myapp { inherit pkgs; };
    git = pkgs.git;
  };
}
```

In this example, we're defining two outputs: `myproject.myapp` and `git`.

## Building outputs

To build all defined outputs, run:

```shell-session
$ devenv build
/nix/store/mzq5bpi49h26cy2mfj5a2r0q69fh3a9k-git-2.44.0
/nix/store/mzq5bpi49h26cy2mfj5a2r0q69fh3a9k-myapp-1.0
```

This command will build all outputs and display their paths in the Nix store.

To build specific output(s), you can specify them explicitly:

```shell-session
$ devenv build outputs.git
/nix/store/mzq5bpi49h26cy2mfj5a2r0q69fh3a9k-git-2.44.0
```

This will build only the `git` output, making it easy to consume for installation or distribution.

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
