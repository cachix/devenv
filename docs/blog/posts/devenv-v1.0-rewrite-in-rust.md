---
draft: false 
date: 2023-03-19
authors:
  - domenkozar
---

# devenv 1.0: Rewrite in Rust

We've just [released devenv 1.0](https://devenv.sh/)! 🎉

This is a rewrite of the CLI to ~~[Python](https://github.com/cachix/devenv/pull/745)~~ [Rust](https://github.com/cachix/devenv/pull/1005),
which brings a lot of new features and improvements.

I'd like to thank [mightyiam](https://app.reclaim.ai/m/mightyiam/flexible) for a week long
pair Rust programming session at [Thaiger Sprint](https://thaigersprint.org).

Note to read the migration guide at the end of this post, as 1.0 is not entirely backwards compatible.

## Why rewrite twice?

When I started to write this blog post for the Python rewrite,
I came up with only excuses why it's not fast and realized that we're breaking our promise.

The seconds reason is that in the Nix community there was a lot of controversy around flakes (that's for another blog post)
and [tvix](https://tvix.dev/) people decided to do something about it and rewrite Nix in Rust.

This leaves us the opportunity to use the same libraries and tools as tvix in the future.

## What's new?

There are many contributions in this release spanning over a year, but here are some of the highlights:

### process-compose is now the default process manager

`devenv up` is now using [process-compose](https://github.com/F1bonacc1/process-compose),
as it handles dependencies between processes and provides a nice ncurses interface to see all processes
and their logs.

### Testing infrastructure

Testing has been a major focus of this release, and we've added a number of features to make it easier to write and run tests.

There's a new `enterTest` attribute in `devenv.nix` that allows you to define testing logic:

```nix
{ pkgs, ... }: {
  packages = [ pkgs.ncdu ];

  enterTest = ''
    ncdu --version | grep "ncdu 2.2"
  '';
}
```

When you run `devenv test`, it will run the `enterTest` command and report the results.

If you have any [processes](/processes) defined, they will be started and stopped.

Read more about this in the [testing documentation](/tests).


### devenv-nixpkgs

We run about 300 tests across different languages and processes to make sure to catch any regressions.

Since [nixpkgs-unstable](https://status.nixos.org/) has fairly few tests,
we've created [devenv-nixpkgs](https://github.com/cachix/devenv-nixpkgs) to run tests on top of nixpkgs-unstable
and apply any patches we're upstreaming to address any issues.

### non-root containers

Generated containers now run as a plain user,
which is a security improvement and as well unlocks running software that forbids root.

### DEVENV_RUNTIME

Due to [socket path limits](https://github.com/cachix/devenv/issues/540), we've introduced `DEVENV_RUNTIME` environment variable that points to `$XDG_RUNTIME_DIR` and falls back to `/tmp`.

### Python has first-class support for native libraries

This one was the hardest nut to crack.

Nix is known to have poor experience using tools like pip.

We've put in a lot of effort to make it finally be possible to use native libraries in Python without any extra effort.

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

To update only one input:

`devenv update <input>`

To build any attribute in your `devenv.nix`:

`devenv build languages.rust.package`

To run the environment with as clean as possible environment but keeping some variables:  

`devenv shell --clean EDITOR,PAGER`

We've also tweaked default number of cores to 2 and max-jobs to number of cpus divided by two.
It's impossible to find an ideal default, but we've found that too much parallelism hurts performance and
running out of memory is a common issue.

... and a number of other additions:

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

### Deprecations:

* `devenv container --copy <name>` has been renamed to `devenv container copy <name>`.
* `devenv container --docker-run <name>` has been renamed to `devenv container run <name>`.
* `devenv ci` has been renamed to `devenv test` with a broader scope.

### Breaking changes

* `.env` files must start with `.env` prefix
* We've finally removed the need for `--impure` flag, which means that devenv is now fully hermetic by default.

  Things like `builtins.currentSystem` won't work anymore, you'll have to use `pkgs.stdenv.system`.
  
  If you need to relax the hermeticity of the environment, you can use `devenv shell --impure`.
* Since `devenv.lock` changed format, once you generate it you can not use it with older versions of devenv.

## Looking ahead

There are a number of features we're looking to add in the future, please vote on the issues:

### Running devenv in a container

While devenv is designed to be run on your local machine, we're looking to add support for [running devenv inside a container](https://github.com/cachix/devenv/issues/1010).

Something like:

```
devenv shell --in-container
devenv test --in-container
```

This is convenient when the environment is too complex to set up on your local machine (for example, running two databases) or when you want to run tests in a clean environment.

### Generating containers with full environment

Currently `enterShell` is executed once the container start,
if we want to execute it as part of the container generation, we have
to [execute it inside a container to generate a layer](https://github.com/cachix/devenv/issues/997).

### macOS support for generating containers

Currently it's only possible to build containers on macOS,
but [it should be possible](https://github.com/cachix/devenv/issues/997).

### Native mapping of dependencies

Would it be cool if devenv could map language specific dependencies to your local system?

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

Should know that `pillow` requires `pkgs.cario`.

### Voilà

If you give devenv a try, hop on [our discord](https://discord.com/invite/naMgvexb6q) and let us know how it goes!

Domen