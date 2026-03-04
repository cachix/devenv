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

[![Built with devenv](https://devenv.sh/assets/devenv-badge.svg)](https://devenv.sh)
[![Built with Nix](https://img.shields.io/static/v1?logo=nixos&logoColor=white&label=&message=Built%20with%20Nix&color=41439a)](https://builtwithnix.org)
[![Discord channel](https://img.shields.io/badge/dynamic/json?url=https%3A%2F%2Fdiscord.com%2Fapi%2Finvites%2FnaMgvexb6q%3Fwith_counts%3Dtrue&query=%24.approximate_member_count&logo=discord&logoColor=white&label=Discord%20users&color=green&style=flat)](https://discord.gg/naMgvexb6q)
![License: Apache 2.0](https://img.shields.io/github/license/cachix/devenv)
[![Version](https://img.shields.io/github/v/release/cachix/devenv?color=green&label=version&sort=semver)](https://github.com/cachix/devenv/releases)
[![CI](https://github.com/cachix/devenv/actions/workflows/release.yml/badge.svg)](https://github.com/cachix/devenv/actions/workflows/release.yml?branch=main)

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
https://devenv.sh 2.0.0: Fast, Declarative, Reproducible, and Composable Developer Environments

Usage: devenv [OPTIONS] [COMMAND]

Commands:
  init        Scaffold devenv.yaml, devenv.nix, and .gitignore.
  generate    Generate devenv.yaml and devenv.nix using AI
  shell       Activate the developer environment. https://devenv.sh/basics/
  update      Update devenv.lock from devenv.yaml inputs. http://devenv.sh/inputs/
  search      Search for packages and options in nixpkgs. https://devenv.sh/packages/#searching-for-a-file
  info        Print information about this developer environment.
  up          Start processes in the foreground. https://devenv.sh/processes/
  processes   Start or stop processes. https://devenv.sh/processes/
  tasks       Run tasks. https://devenv.sh/tasks/
  test        Run tests. http://devenv.sh/tests/
  container   Build, copy, or run a container. https://devenv.sh/containers/
  inputs      Add an input to devenv.yaml. https://devenv.sh/inputs/
  changelogs  Show relevant changelogs.
  repl        Launch an interactive environment for inspecting the devenv configuration.
  gc          Delete previous shell generations. See https://devenv.sh/garbage-collection
  build       Build any attribute in devenv.nix.
  eval        Evaluate any attribute in devenv.nix and return JSON.
  direnvrc    Print a direnvrc that adds devenv support to direnv. See https://devenv.sh/integrations/direnv/.
  version     Print the version of devenv.
  mcp         Launch Model Context Protocol server for AI assistants
  lsp         Start the nixd language server for devenv.nix.
  help        Print this message or the help of the given subcommand(s)

Input overrides:
      --from <FROM>
          Source for devenv.nix.

          Can be either a filesystem path (with path: prefix) or a flake input reference.

          Examples:
            --from github:cachix/devenv
            --from github:cachix/devenv?dir=examples/simple
            --from path:/absolute/path/to/project
            --from path:./relative/path

  -o, --override-input <NAME> <URI>
          Override inputs in devenv.yaml.

          Examples:
            --override-input nixpkgs github:NixOS/nixpkgs/nixos-unstable
            --override-input nixpkgs path:/path/to/local/nixpkgs

  -O, --option <OPTION:TYPE> <VALUE>
          Override configuration options with typed values.

          OPTION must include a type: <attribute>:<type>
          Supported types: string, int, float, bool, path, pkg, pkgs

          List types (pkgs) append to existing values by default.
          Add a ! suffix to replace instead: pkgs!

          Examples:
            --option languages.rust.channel:string beta
            --option services.postgres.enable:bool true
            --option languages.python.version:string 3.10
            --option packages:pkgs "ncdu git"       (appends to packages)
            --option packages:pkgs! "ncdu git"      (replaces all packages)

Nix options:
  -j, --max-jobs <MAX_JOBS>
          Maximum number of Nix builds to run concurrently.

          Defaults to 1/4 of available CPU cores (minimum 1).

          [env: DEVENV_MAX_JOBS=]

  -u, --cores <CORES>
          Number of CPU cores available to each build.

          Defaults to available cores divided by max-jobs (minimum 1).

          [env: DEVENV_CORES=]

  -s, --system <SYSTEM>
          Override the target system.

          Defaults to the host system (e.g. aarch64-darwin, x86_64-linux).

  -i, --impure
          Relax the hermeticity of the environment.

      --no-impure
          Force a hermetic environment, overriding config.

      --offline
          Disable substituters and consider all previously downloaded files up-to-date.

      --nix-option <NAME> <VALUE>
          Pass additional options to nix commands.

          These options are passed directly to Nix using the --option flag.
          See `man nix.conf` for the full list of available options.

          Examples:
            --nix-option sandbox false
            --nix-option keep-outputs true
            --nix-option system x86_64-darwin

      --nix-debugger
          Enter the Nix debugger on failure.

Shell options:
  -c, --clean [<CLEAN>...]
          Ignore existing environment variables when entering the shell. Pass a list of comma-separated environment variables to let through.

  -P, --profile <PROFILES>
          Activate one or more profiles defined in devenv.nix.

          Profiles allow you to define different configurations that can be merged with your base configuration.

          See https://devenv.sh/profiles for more information.

          Examples:
            --profile python-3.14
            --profile backend --profile fast-startup

      --reload
          Enable auto-reload when config files change (default).

      --no-reload
          Disable auto-reload when config files change.

Cache options:
      --eval-cache
          Enable caching of Nix evaluation results (default).

      --no-eval-cache
          Disable caching of Nix evaluation results.

      --refresh-eval-cache
          Force a refresh of the Nix evaluation cache.

      --refresh-task-cache
          Force a refresh of the task cache.

Secretspec options:
      --secretspec-provider <SECRETSPEC_PROVIDER>
          Override the secretspec provider

          [env: SECRETSPEC_PROVIDER=]

      --secretspec-profile <SECRETSPEC_PROFILE>
          Override the secretspec profile

          [env: SECRETSPEC_PROFILE=]

Tracing options:
      --trace-output <TRACE_OUTPUT>
          Enable tracing and set the output destination: stdout, stderr, or file:<path>. Tracing is disabled by default.

          [env: DEVENV_TRACE_OUTPUT=]

      --trace-format <TRACE_FORMAT>
          Set the trace output format. Only takes effect when tracing is enabled via --trace-output.

          Possible values:
          - full:   A verbose structured log format used for debugging
          - json:   A JSON log format used for machine consumption
          - pretty: A pretty human-readable log format used for debugging

          [env: DEVENV_TRACE_FORMAT=]
          [default: json]

Global options:
  -v, --verbose
          Enable additional debug logs.

  -q, --quiet
          Silence all logs

      --tui
          Enable the interactive terminal interface (default when interactive).

          [env: DEVENV_TUI=]

      --no-tui
          Disable the interactive terminal interface.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version information and exit
```

## Documentation

- [Getting Started](https://devenv.sh/getting-started/)
- [Basics](https://devenv.sh/basics/)
- [Roadmap](https://devenv.sh/roadmap/)
- [Blog](https://devenv.sh/blog/)
- [`devenv.yaml` reference](https://devenv.sh/reference/yaml-options/)
- [`devenv.nix` reference](https://devenv.sh/reference/options/)
- [Contributing](https://devenv.sh/community/contributing/)
