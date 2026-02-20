Join our community on [Discord](https://discord.gg/naMgvexb6q) to discuss the development of `devenv`.

When contributing, please note that smaller pull requests have a higher chance of being accepted, and pull requests with tests will be prioritized.

We have a rule that new features need to come with documentation and tests (`devenv-run-tests`) to ensure the project stays healthy.

## Preparing the `devenv` development environment

1. Follow the [installation instructions for Nix and Cachix](../getting-started.md#installation) and [install direnv](../integrations/direnv.md).

2. `git clone https://github.com/cachix/devenv.git`

3. `cd devenv`

4. To build the project, run `direnv allow .` or build devenv manually using
   `nix build .#devenv` which allows to run development version of devenv outside
   of source code directory by calling `<PATH-TO-DEVENV-SOURCE-CODE>/result/bin/devenv`.

## Repository structure

- The project is a Cargo workspace with multiple crates:
  - `devenv/` - Main CLI binary. Entry point is `devenv/src/main.rs`, command dispatch in `devenv/src/devenv.rs`, CLI definitions in `devenv/src/cli.rs`.
  - `devenv-core/` - Shared types: configuration parsing, `NixBackend` trait, global options.
  - `devenv-tasks/` - DAG-based task execution with caching and parallel execution.
  - `devenv-eval-cache/` - SQLite-based Nix evaluation cache.
  - `devenv-tui/` - Terminal UI for build progress.
  - `devenv-run-tests/` - Integration test harness.
- All Nix modules related to `devenv.nix` are in `src/modules/` (`languages/`, `services/`, `integrations/`, `process-managers/`). New modules placed in these directories are auto-discovered.
- Examples are automatically tested on CI and are the best way to work on developing new modules, see `examples/` and `tests/`.
- Documentation is in `docs/`. To run a documentation dev server, run `devenv up`.
- To run a test from `examples/` or `tests/`, run `devenv-run-tests run tests --only <name>`.

## Building and testing the CLI

- `cargo build` - Build the CLI.
- `cargo test` or `cargo nextest run` - Run unit tests.
- `cargo fmt` - Format code.
- `cargo clippy` - Lint code.

## Adding changelogs for breaking and behavior changes

When making breaking changes or important behavior changes that affect users, add a changelog entry so they are informed after running `devenv update`.

Changelogs are defined in any `devenv.nix` module or configuration using the `changelogs` option:

```nix
{
  changelogs = [
    {
      date = "2025-01-15";
      title = "git-hooks.package is now pkgs.prek";
      when = config.git-hooks.enable;  # Condition for showing this changelog
      description = ''
        The git-hooks.package option now defaults to pkgs.prek instead of pkgs.pre-commit.
        If you were using a custom package, please update your configuration.
      '';
    }
  ];
}
```

Each changelog entry requires:
- `date`: A YYYY-MM-DD formatted date string
- `title`: A short description of the breaking change or behavior change
- `when`: A boolean condition for when to show this changelog (e.g., based on whether a feature is enabled)
- `description`: A markdown-formatted detailed description of the change and any migration steps

Changelogs are deduplicated based on date and title, so you can safely update the description without affecting deduplication. Users can view all relevant changelogs with the `devenv changelogs` command.

## Contributing service improvements

To add a new service module under `src/modules/services/`, follow the [`create-service` skill](https://github.com/cachix/devenv/blob/main/.agents/skills/create-service/SKILL.md). It documents the conventions for port allocation, readiness probes, socket activation, and setup tasks.

## Contributing language improvements

Language integration happens in stages. We welcome even the most basic support for getting started.

The most basic language support starts with the `languages.*.enable` flag, which turns on basic tooling.
For an example, see `src/modules/languages/elm.nix`.

The next step is to make the tooling customizable, so the versions can be overridden.
Most languages will come with either a `languages.*.package` or `languages.*.packages` option that allows the user to customize what version or package of the language they want to pick.

A further step is to provide `languages.*.version` option, which allows the user to specify the exact version of the language.
For an example, see `src/modules/languages/rust.nix`.
