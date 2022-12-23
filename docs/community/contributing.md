Join our community on [Discord](https://discord.gg/naMgvexb6q) to discuss the development of devenv.

When contributing, please note that smaller pull requests have a higher chance of being accepted and those with tests will be prioritized.

We have the rule that new features need to come with documentation and tests (`devenv-run-tests`)
This is to ensure the project stays healthy.

## Preparing `devenv` development environment

1. Follow [Installation instructions for Nix and Cachix](../../getting-started/#installation).

2. `git clone https://github.com/cachix/devenv.git`

3. `cd devenv`

4. To build the project run `nix-build`

5. `./result/bin/devenv shell`

6. Once you make any changes, run `./result/bin/devenv shell` again.

To automate this workflow [install and use direnv](../../automatic-shell-activation/).

## Repository structure

- `devenv` CLI is in `src/devenv.nix`.
- `flake.nix` auto-generation logic lies in `src/flake.nix`.
- All modules related to `devenv.nix` are in `src/modules/`.
- Examples get automatically tested on CI and are the best way to work on developing new modules, see `examples/`.
- Documentation is in `docs/` and to run a development server run `devenv up`.

## Contributing language improvements

Language integration happens in stages, we welcome even most the basic support for getting started.

The most basic language support starts with the `languages.*.enable` flag,
which turns on basic tooling. For an example see `src/modules/languages/elm.nix`.

The next step is to make the tooling customizable, so the versions can be overriden.
Most languages will come with either `languages.*.package` or `languages.*.packages` option
that allows the user to customize what version/package of the language they want to pick.

A further step is to provide `languages.*.version` option, which allows the user to specify the exact version of the language.
For an example see `src/modules/languages/rust.nix`.
