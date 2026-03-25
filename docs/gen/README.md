# Documentation Generation

A standalone devenv environment that generates documentation for [devenv.sh] by extracting and formatting options from the devenv module system.

## Scripts

### `devenv-generate-doc-options`

Generates the complete options reference at [`/docs/src/reference/options.md`](../src/reference/options.md).

### `devenv-generate-option-docs`

Generates standalone option docs for each language, service, and process manager into [`/docs/src/_generated/`](../src/_generated/).
These are inlined into the human-written docs via `pymdownx.snippets` at mkdocs build time.

### `devenv-verify-module-docs`

Creates missing documentation files in [`/docs/src/`](../src/) when new modules are added.

### `devenv-generate-docs`

Generates code snippets listing all available languages and services in [`/docs/src/snippets/`](../src/snippets/).

## Configuration

### Adding optional modules

If you add an optional module with documentation (an input that exposes submodules), add it as an input in [`devenv.yaml`](devenv.yaml).
This ensures its options appear in the generated documentation.

## Workflow

1. Make changes to modules in [`/src/modules/`](../../src/modules/)
2. Run `devenv-verify-module-docs` to create doc files for new modules
3. Add guides and examples in [`/docs/src/languages/`](../src/languages/), [`/docs/src/services/`](../src/services/), etc.
4. Run `devenv-generate-option-docs` to populate [`/docs/src/_generated/`](../src/_generated/) before serving docs

[devenv.sh]: https://devenv.sh/
