<p align="center">
  <a href="https://devenv.sh">
    <picture>
      <source media="(prefers-color-scheme: light)" srcset="logos/devenv-horizontal-light-bg.svg">
      <source media="(prefers-color-scheme: dark)" srcset="logos/devenv-horizontal-dark-bg.svg">
      <img src="logos/devenv-horizontal-light-bg.svg" width="500px" alt="devenv logo">
    </picture>
  </a>
</p>

# [devenv.sh](https://devenv.sh) - Fast, Declarative, Reproducible, and Composable Developer Environments

[![Built with Nix](https://img.shields.io/static/v1?logo=nixos&logoColor=white&label=&message=Built%20with%20Nix&color=41439a)](https://builtwithnix.org)
[![Discord channel](https://img.shields.io/badge/dynamic/json?url=https%3A%2F%2Fdiscord.com%2Fapi%2Finvites%2FnaMgvexb6q%3Fwith_counts%3Dtrue&query=%24.approximate_member_count&logo=discord&logoColor=white&label=Discord%20users&color=green&style=flat)](https://discord.gg/naMgvexb6q)
![License: Apache 2.0](https://img.shields.io/github/license/cachix/devenv)
[![Version](https://img.shields.io/github/v/release/cachix/devenv?color=green&label=version&sort=semver)](https://github.com/cachix/devenv/releases)
[![CI](https://github.com/cachix/devenv/actions/workflows/buildtest.yml/badge.svg)](https://github.com/cachix/devenv/actions/workflows/buildtest.yml?branch=main)

Running ``devenv init`` generates ``devenv.nix``:

```nix
{ pkgs, lib, config, inputs, ... }:

{
  # https://devenv.sh/basics/
  env.GREET = "devenv";

  # https://devenv.sh/packages/
  packages = [ pkgs.git ];

  # https://devenv.sh/languages/
  # languages.rust.enable = true;

  # https://devenv.sh/processes/
  # processes.dev.exec = "${lib.getExe pkgs.watchexec} -n -- ls -la";

  # https://devenv.sh/services/
  # services.postgres.enable = true;

  # https://devenv.sh/scripts/
  scripts.hello.exec = ''
    echo hello from $GREET
  '';

  # https://devenv.sh/basics/
  enterShell = ''
    hello         # Run scripts directly
    git --version # Use packages
  '';

  # https://devenv.sh/tasks/
  # tasks = {
  #   "myproj:setup".exec = "mytool build";
  #   "devenv:enterShell".after = [ "myproj:setup" ];
  # };

  # https://devenv.sh/tests/
  enterTest = ''
    echo "Running tests"
    git --version | grep --color=auto "${pkgs.git.version}"
  '';

  # https://devenv.sh/outputs/
  # outputs = {
  #   rust-app = config.languages.rust.import ./rust-app {};
  #   python-app = config.languages.python.import ./python-app {};
  # };

  # https://devenv.sh/git-hooks/
  # git-hooks.hooks.shellcheck.enable = true;

  # See full reference at https://devenv.sh/reference/options/
}

```

And ``devenv shell`` activates the environment.

## Commands

```
$ devenv
https://devenv.sh 1.9.0: Fast, Declarative, Reproducible, and Composable Developer Environments

Usage: devenv [OPTIONS] [COMMAND]

Commands:
  init       Scaffold devenv.yaml, devenv.nix, .gitignore and .envrc.
  generate   Generate devenv.yaml and devenv.nix using AI
  shell      Activate the developer environment. https://devenv.sh/basics/
  update     Update devenv.lock from devenv.yaml inputs. http://devenv.sh/inputs/
  search     Search for packages and options in nixpkgs. https://devenv.sh/packages/#searching-for-a-file
  info       Print information about this developer environment.
  up         Start processes in the foreground. https://devenv.sh/processes/
  processes  Start or stop processes. https://devenv.sh/processes/
  tasks      Run tasks. https://devenv.sh/tasks/
  test       Run tests. http://devenv.sh/tests/
  container  Build, copy, or run a container. https://devenv.sh/containers/
  inputs     Add an input to devenv.yaml. https://devenv.sh/inputs/
  repl       Launch an interactive environment for inspecting the devenv configuration.
  gc         Delete previous shell generations. See https://devenv.sh/garbage-collection
  build      Build any attribute in devenv.nix.
  direnvrc   Print a direnvrc that adds devenv support to direnv. See https://devenv.sh/automatic-shell-activation.
  version    Print the version of devenv.
  mcp        Launch Model Context Protocol server for AI assistants
  help       Print this message or the help of the given subcommand(s)

Options:
  -V, --version
          Print version information and exit

  -v, --verbose
          Enable additional debug logs.

  -q, --quiet
          Silence all logs

      --log-format <LOG_FORMAT>
          Configure the output format of the logs.
          
          [default: cli]

          Possible values:
          - cli:            The default human-readable log format used in the CLI
          - tracing-full:   A verbose structured log format used for debugging
          - tracing-pretty: A pretty human-readable log format used for debugging

  -j, --max-jobs <MAX_JOBS>
          Maximum number of Nix builds at any time.
          
          [default: 8]

  -u, --cores <CORES>
          Maximum number CPU cores being used by a single build.
          
          [default: 2]

  -s, --system <SYSTEM>
          [default: x86_64-linux]

  -i, --impure
          Relax the hermeticity of the environment.

      --no-eval-cache
          Disable caching of Nix evaluation results.

      --refresh-eval-cache
          Force a refresh of the Nix evaluation cache.

      --offline
          Disable substituters and consider all previously downloaded files up-to-date.

  -c, --clean [<CLEAN>...]
          Ignore existing environment variables when entering the shell. Pass a list of comma-separated environment variables to let through.

      --nix-debugger
          Enter the Nix debugger on failure.

  -n, --nix-option <NAME> <VALUE>
          Pass additional options to nix commands.
          
          These options are passed directly to Nix using the --option flag.
          See `man nix.conf` for the full list of available options.
          
          Examples:
            --nix-option sandbox false
            --nix-option keep-outputs true
            --nix-option system x86_64-darwin

  -o, --override-input <NAME> <URI>
          Override inputs in devenv.yaml.
          
          Examples:
            --override-input nixpkgs github:NixOS/nixpkgs/nixos-unstable
            --override-input nixpkgs path:/path/to/local/nixpkgs

  -O, --option <OPTION> <VALUE>
          Override configuration options with typed values.
          
          OPTION must include a type: <attribute>:<type>
          Supported types: string, int, float, bool, path, pkg, pkgs
          
          Examples:
            --option languages.rust.channel:string beta
            --option services.postgres.enable:bool true
            --option languages.python.version:string 3.10
            --option packages:pkgs "ncdu git"

  -P, --profile <PROFILE>
          Activate one or more profiles defined in devenv.nix.
          
          Profiles allow you to define different configurations that can be merged with your base configuration.
          
          See https://devenv.sh/profiles for more information.
          
          Examples:
            --profile python-3.14
            --profile backend --profile fast-startup

  -h, --help
          Print help (see a summary with '-h')
```

## Documentation

- [Getting Started](https://devenv.sh/getting-started/)
- [Basics](https://devenv.sh/basics/)
- [Roadmap](https://devenv.sh/roadmap/)
- [Blog](https://devenv.sh/blog/)
- [`devenv.yaml` reference](https://devenv.sh/reference/yaml-options/)
- [`devenv.nix` reference](https://devenv.sh/reference/options/)
- [Contributing](https://devenv.sh/community/contributing/)
