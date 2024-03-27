Join our community on [Discord](https://discord.gg/naMgvexb6q) to discuss the development of `devenv`.

When contributing, please note that smaller pull requests have a higher chance of being accepted, and pull requests with tests will be prioritized.

We have a rule that new features need to come with documentation and tests (`devenv-run-tests`) to ensure the project stays healthy.

## Preparing the `devenv` development environment

1. Follow the [installation instructions for Nix and Cachix](../../getting-started/#installation) and [install direnv](../../automatic-shell-activation/).

2. `git clone https://github.com/cachix/devenv.git`

3. `cd devenv`

4. To build the project, run `direnv allow .` or build devenv manually using
`nix build .#devenv` which allows to run development version of devenv outside
of source code directory by calling `<PATH-TO-DEVENV-SOURCE-CODE>/result/bin/devenv`.

## Creating development project

1. `mkdir devenv-project && cd devenv-project`

2. `<PATH-TO-DEVENV-SOURCE-CODE>/result/bin/devenv init`

3. Add devenv input pointing to local source directory to `devenv.yaml`
  ```
  devenv:
    url: path:<PATH-TO-DEVENV-SOURCE-CODE>?dir=src/modules
  ```

4. `<PATH-TO-DEVENV-SOURCE-CODE>/result/bin/devenv update`

## Repository structure

- The `devenv` CLI is in `devenv/src/main.rs`.
- The `flake.nix` auto-generation logic lies in `devenv/src/flake.tmpl.nix`.
- All modules related to `devenv.nix` are in `src/modules/`.
- Examples are automatically tested on CI and are the best way to work on developing new modules, see `examples/` and `tests/`
- Documentation is in `docs/`.
- To run a development server, run `devenv up`.
- To run a test, run `devenv-run-tests --only <example-name> examples`.

## Contributing language improvements

Language integration happens in stages. We welcome even the most basic support for getting started.

The most basic language support starts with the `languages.*.enable` flag, which turns on basic tooling. 
For an example, see `src/modules/languages/elm.nix`.

The next step is to make the tooling customizable, so the versions can be overridden.
Most languages will come with either a `languages.*.package` or `languages.*.packages` option that allows the user to customize what version or package of the language they want to pick.

A further step is to provide `languages.*.version` option, which allows the user to specify the exact version of the language.
For an example, see `src/modules/languages/rust.nix`.
