# `devenv` Integration for [treefmt](https://treefmt.com/) via [treefmt-nix](https://github.com/numtide/treefmt-nix)

## Set up

Check the available integrations in [the list of all available integrations](/reference/options.md#treefmtenable).

Add your desired integration to your `devenv.nix` file. For example, the following code would enable `treefmt` with the `nixpkgs-fmt` and `rustfmt` integrations:

```nix
{ inputs, ... }:

{
  treefmt = {
    enable = true;
    config.programs = {
      nixfmt.enable = true;
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

### Git Hooks

If you would like to enable `treefmt` in your git-hooks hooks, simply add:

```nix
{ inputs, ... }:

{
  git-hooks.hooks = {
    treefmt.enable = true;
  };
}
```

This will enable `treefmt` hooks and automatically change the default package to the one you have defined in your `devenv`.

## Using a Custom Formatter

It is also possible to use custom formatters with `treefmt-nix`. For example, the following custom formatter formats JSON files using `yq-go`:

```nix
{ pkgs, lib, ... }

{
  treefmt.config.settings.formatter = {
    "yq-json" = {
      command = "${lib.getExe pkgs.bash}";
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
      excludes = [ ".git/*" ".devenv/*" ]; # don't mess with git and devenv files
    };
  };
}
```
