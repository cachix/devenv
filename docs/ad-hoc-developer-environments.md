# Ad-hoc Developer Environments

!!! info "New in 1.6"

Instead of creating and maintaining a `devenv.nix` file, you can create ad-hoc developer environments directly from the command line using the `--option` flag.

## Basic Usage

You can specify any configuration option using the `--option` flag, allowing you to create temporary environments with specific tools and settings:

```shell-session
$ devenv --option languages.python.enable:bool true \
         --option languages.python.version:string "3.10" \
         shell
```

This creates a temporary Python development environment without needing any configuration files.

## Option Types

The `--option` flag requires you to specify the inferred Nix type:

- `:string` for string values
- `:int` for integer values
- `:float` for floating-point values  
- `:bool` for boolean values (true/false)
- `:path` for file paths (interpreted as relative paths)
- `:pkgs` for lists of packages (space-separated package names)

## Installing Packages

To install packages from nixpkgs, use the special `:pkgs` type with the `packages` option:

```shell-session
$ devenv --option packages:pkgs "ncdu git ripgrep" shell
```

This creates an environment with `ncdu`, `git`, and `ripgrep` available, without needing a `devenv.nix` file.

## Enabling Languages and Services

You can enable languages and services with ad-hoc environments:

```shell-session
$ devenv --option languages.rust.enable:bool true \
         --option languages.rust.channel:string nightly \
         shell
```

## Running Ad-hoc Commands

You can also run specific commands directly in your ad-hoc environment:

```shell-session
$ devenv --option languages.elixir.enable:bool true shell iex
```

This example launches an Elixir interactive shell (`iex`) immediately after creating the environment.

## Combining with `devenv.nix`

When used with an existing `devenv.nix` file, `--option` values will override the configuration in the file.

## Use Cases

Ad-hoc environments are particularly useful for:

1. Quick testing of languages or tools
2. One-off tasks that need specific dependencies
3. Trying different configurations before committing to a `devenv.nix` file
4. Creating lightweight environments without project setup
5. Creating a matrix of different options, e.g. testing different versions of Python

## Limitations

While ad-hoc environments are convenient, they have some limitations:

- Complex configurations are better managed in a `devenv.nix` file
- Some complex options may be harder to express on the command line
