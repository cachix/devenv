# `devenv` Integration for [treefmt](https://treefmt.com/) via [treefmt-nix](https://github.com/numtide/treefmt-nix)

## Set up

Check the available integrations in [the list of all available integrations](reference/options.md#treefmt).

Add your desired integration to your `devenv.nix` file. For example, the following code would enable `treefmt` with the `nixpkgs-fmt` and `rustfmt` integrations:

```nix
{ inputs, ... }:

{
  treefmt = {
    projectRootFile = "devenv.nix";
    programs = {
        nixpkgs-fmt.enable = true;
        rustfmt.enable = true;
    };
  };
}
```

### In Action:

```shell-session
$ devenv shell
Building shell ...
Entering shell ...

treefmt # This would run treefmt on all files.
```

## Additional Devenv Integrations

### Pre-commit Hooks

If you would like to enable `treefmt` in your pre-commit hooks, simply add:

```nix
{ inputs, ... }:

{
  pre-commit.hooks = {
    treefmt.enable = true;
  };
}
```

This will enable `treefmt` hooks and automatically change the default package to the one you have defined in your `devenv`.

### Just

You can also enable the `just` command `just fmt` to run `treefmt`. To do so, add the following to your `devenv.nix`:

```nix
{ inputs, ... }:

{
  just = {
    enable = true;
    recipes = {
        treefmt.enable = true;
    };
  };
}
```

## Using a Custom Formatter

It is also possible to use custom formatters with `treefmt-nix`. For example, the following custom formatter formats JSON files using `yq-go`:

```nix
{
  treefmt.settings.formatter = {
    "yq-json" = {
      command = "${pkgs.bash}/bin/bash";
      options = [
        "-euc"
        ''
          for file in "$@"; do
            ${lib.getExe yq-go} -i --output-format=json $file
          done
        ''
        "--" # bash swallows the second argument when using -c
      ];
      includes = [ "*.json" ];
    };
  };
}
```
