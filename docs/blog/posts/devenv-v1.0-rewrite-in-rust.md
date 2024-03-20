---
draft: false
date: 2024-03-20
authors:
  - domenkozar
---

# devenv 1.0: Rewrite in Rust

We have just released [devenv 1.0](https://devenv.sh/)! 🎉

This is a rewrite of the CLI to ~~[Python](https://github.com/cachix/devenv/pull/745)~~ [Rust](https://github.com/cachix/devenv/pull/1005),
which brings with it many new features and improvements.

I would like to thank [mightyiam](https://app.reclaim.ai/m/mightyiam/flexible) for a week-long, Rust pair-programming session at [Thaiger Sprint](https://thaigersprint.org).

Note: Read the migration guide at the end of this post, as 1.0 is not entirely backwards compatible.

## Why rewrite twice?

When I started to write this blog post for the Python rewrite, I came up with only excuses as to why it is not fast and realized that we were simply breaking our promise to you.

The second reason is that in the Nix community there has been a lot of controversy surrounding flakes (that's for another blog post); two years ago, the [tvix](https://tvix.dev/) developers decided to do something about it and started a rewrite of Nix in Rust. This leaves us with the opportunity in the future to use the same Rust libraries and tooling.

## What's new?

There are many contributions in this release, spanning over a year, but here are some of the highlights:

### process-compose is now the default process manager

`devenv up` is now using [process-compose](https://github.com/F1bonacc1/process-compose),
as it handles dependencies between processes and provides a nice ncurses interface to view the processes
and their logs.

### Testing infrastructure

Testing has been a major focus of this release, and a number of features have been added to make it easier to write and run tests.

The new `enterTest` attribute in `devenv.nix` allows you to define testing logic:

```nix
{ pkgs, ... }: {
  packages = [ pkgs.ncdu ];

  services.postgres = {
    enable = true;
    listen_addresses = "127.0.0.1";
    initialDatabases = [{ name = "mydb"; }];
  };

  enterTest = ''
    wait_for_port 5432
    ncdu --version | grep "ncdu 2.2"
  '';
}
```

When you run `devenv test`, it will run the `enterTest` command and report the results.

If you have any [processes](/processes) defined, they will be started and stopped.

Read more about this in the [testing documentation](/tests).

This allows for executing tests with all of your tooling and processes running—extremely convenient for integration and functional tests.

### devenv-nixpkgs

Since [nixpkgs-unstable](https://status.nixos.org/) has fairly few tests,
we have created [devenv-nixpkgs](https://github.com/cachix/devenv-nixpkgs) to run tests on top of `nixpkgs-unstable`—applying patches we are upstreaming to address any issues.

We run around 300 tests across different languages and processes to ensure all regressions are caught.

### Non-root containers

Generated containers now run as a plain user—improving security and unlocking the ability to run software that forbids root.

### DEVENV_RUNTIME

Due to [socket path limits](https://github.com/cachix/devenv/issues/540), the `DEVENV_RUNTIME` environment variable has been introduced: pointing to `$XDG_RUNTIME_DIR` by default and falling back to `/tmp`.

### First-class support for Python native libraries

This one was the hardest nut to crack.

Nix is known to provide a poor experience when using tools like pip.

A lot of work has been put in here, finally making it possible to use native libraries in Python without any extra effort:

```nix
{ pkgs, lib, ... }: {
  languages.python = {
    enable = true;
    venv.enable = true;
    venv.requirements = ''
      pillow
    '';
    libraries = [ pkgs.cairo ];
  };
}
```

### CLI improvements

If you need to add an input to `devenv.yaml`, you can now do:

`devenv inputs add <name> <url>`

To update a single input:

`devenv update <input>`

To build any attribute in `devenv.nix`:

`devenv build languages.rust.package`

To run the environment as cleanly as possible while keeping specific variables:

`devenv shell --clean EDITOR,PAGER`

The default number of cores has been tweaked to 2, and `max-jobs` to half of the number of CPUs.
It is impossible to find an ideal default, but we have found that too much parallelism hurts performance—running out of memory is a common issue.

... plus a number of other additions:

```
https://devenv.sh 1.0.0: Fast, Declarative, Reproducible, and Composable Developer Environments

Usage: devenv [OPTIONS] <COMMAND>

Commands:
  init       Scaffold devenv.yaml, devenv.nix, .gitignore and .envrc.
  shell      Activate the developer environment. https://devenv.sh/basics/
  update     Update devenv.lock from devenv.yaml inputs. http://devenv.sh/inputs/
  search     Search for packages and options in nixpkgs. https://devenv.sh/packages/#searching-for-a-file
  info       Print information about this developer environment.
  up         Start processes in the foreground. https://devenv.sh/processes/
  processes  Start or stop processes.
  test       Run tests. http://devenv.sh/tests/
  container  Build, copy, or run a container. https://devenv.sh/containers/
  inputs     Add an input to devenv.yaml. https://devenv.sh/inputs/
  gc         Deletes previous shell generations. See http://devenv.sh/garbage-collection
  build      Build any attribute in devenv.nix.
  version    Print the version of devenv.
  help       Print this message or the help of the given subcommand(s)

Options:
  -v, --verbose
          Enable debug log level.
  -j, --max-jobs <MAX_JOBS>
          Maximum number of Nix builds at any time. [default: 8]
  -j, --cores <CORES>
          Maximum number CPU cores being used by a single build.. [default: 2]
  -s, --system <SYSTEM>
          [default: x86_64-linux]
  -i, --impure
          Relax the hermeticity of the environment.
  -c, --clean [<CLEAN>...]
          Ignore existing environment variables when entering the shell. Pass a list of comma-separated environment variables to let through.
  -d, --nix-debugger
          Enter Nix debugger on failure.
  -n, --nix-option <NIX_OPTION> <NIX_OPTION>
          Pass additional options to nix commands, see `man nix.conf` for full list.
  -o, --override-input <OVERRIDE_INPUT> <OVERRIDE_INPUT>
          Override inputs in devenv.yaml.
  -h, --help
          Print help
```

## Migration guide

### Deprecations

- `devenv container --copy <name>` has been renamed to `devenv container copy <name>`.
- `devenv container --docker-run <name>` has been renamed to `devenv container run <name>`.
- `devenv ci` has been renamed to `devenv test` with a broader scope.

### Breaking changes

- `.env` files must start with the `.env` prefix.
- The need for the `--impure` flag has finally been removed, meaning that devenv is now fully hermetic by default.

  Things like `builtins.currentSystem` no longer work—you will have to use `pkgs.stdenv.system`.

  If you need to relax the hermeticity of the environment you can use `devenv shell --impure`.

- Since the format of `devenv.lock` has changed, newly-generated lockfiles cannot be used with older versions of devenv.

## Looking ahead

There are a number of features that we are looking to add in the future—please vote on the issues:

### Running devenv in a container

While devenv is designed to be run on your local machine, we are looking to add support for [running devenv inside a container](https://github.com/cachix/devenv/issues/1010).

Something like:

```
devenv shell --in-container
devenv test --in-container
```

This would be convenient when the environment is too complex to set up on your local machine; for example, when running two databases or when you want to run tests in a clean environment.

### Generating containers with full environment

Currently, `enterShell` is executed only once the container has started.
If we want to execute it as part of the container generation, we have
to [execute it inside a container to generate a layer](https://github.com/cachix/devenv/issues/997).

### macOS support for generating containers

Building containers on macOS is not currently supported,
but it [should be possible](https://github.com/cachix/devenv/issues/997).

### Native mapping of dependencies

Wouldn't it be cool if devenv could map language-specific dependencies to your local system? In this example, devenv should be able to determine that `pillow` requires `pkgs.cairo`:

```nix
{ pkgs, lib, ... }: {
  languages.python = {
    enable = true;
    venv.enable = true;
    venv.requirements = ''
      pillow
    '';
  };
}
```

### Voilà

[Give devenv a try](https://devenv.sh/getting-started/), and [hop on to our discord](https://discord.com/invite/naMgvexb6q) to let us know how it goes!

Domen
