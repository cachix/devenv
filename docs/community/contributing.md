Join our community on [Discord](https://discord.gg/naMgvexb6q) to discuss the development of `devenv`.

When contributing, please note that smaller pull requests have a higher chance of being accepted, and pull requests with tests will be prioritized.

We have a rule that new features need to come with documentation and tests (`devenv-run-tests`) to ensure the project stays healthy.

## Preparing the `devenv` development environment

1. Follow the [installation instructions for Nix and Cachix](../../getting-started/#installation) and [install direnv](../../automatic-shell-activation/).

2. `git clone https://github.com/cachix/devenv.git`

3. `cd devenv`

4. To build the project, run `direnv allow .`.

## Repository structure

- The `devenv` CLI is in `src/devenv/cli.py`.
- The `flake.nix` auto-generation logic lies in `src/modules/flake.tmpl.nix`.
- All modules related to `devenv.nix` are in `src/modules/`.
- Examples are automatically tested on CI and are the best way to work on developing new modules, see `examples/` and `tests/`
- Documentation is in `docs/`.
- To run a development server, run `devenv up`.
- To run a test, run `devnenv test <example-name>`.

## Contributing language improvements

Language integration happens in stages. We welcome even the most basic support for getting started.

The most basic language support starts with the `languages.*.enable` flag, which turns on basic tooling. 
For an example, see `src/modules/languages/elm.nix`.

The next step is to make the tooling customizable, so the versions can be overridden.
Most languages will come with either a `languages.*.package` or `languages.*.packages` option that allows the user to customize what version or package of the language they want to pick.

A further step is to provide `languages.*.version` option, which allows the user to specify the exact version of the language.
For an example, see `src/modules/languages/rust.nix`.
