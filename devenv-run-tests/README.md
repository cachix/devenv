# devenv-run-tests

Test runner for devenv integration tests.
It runs each test in an isolated temporary directory with a clean environment, automatically wiring the local `src/modules` as the `devenv` input.

## Commands

### `run` — Run tests

```bash
# Run all tests in default directories (examples/ and tests/)
devenv-run-tests run

# Run tests in specific directories
devenv-run-tests run path/to/tests another/path
```

#### Filtering

`--only` and `--exclude` accept glob patterns matched against test directory names:

```bash
# Run a single test
devenv-run-tests run tests --only my-test

# Run tests matching a glob pattern
devenv-run-tests run tests --only 'python-*'

# Exclude tests matching a glob pattern
devenv-run-tests run tests --exclude 'slow-*'
```

#### Overriding inputs

Pass `--override-input` (`-o`) to override `devenv.yaml` inputs:

```bash
devenv-run-tests run tests -o nixpkgs github:NixOS/nixpkgs/nixos-unstable
```

### `generate-json` — Generate test metadata

Outputs JSON metadata for all discovered tests (used by CI):

```bash
devenv-run-tests generate-json [directories...]
devenv-run-tests generate-json --all  # include tests unsupported on current system
```

## Writing tests

Each test is a subdirectory inside `tests/` or `examples/` containing:

| File | Required | Description |
|---|---|---|
| `devenv.nix` | yes | The devenv configuration to test |
| `.test.sh` | no | Test script (runs inside `devenv shell` by default) |
| `.test-config.yml` | no | Test configuration (see below) |
| `.setup.sh` | no | Setup script that runs in the shell before the test |
| `.patch.sh` | no | Patch script that runs *before* config is loaded (outside the shell) |

### Test configuration (`.test-config.yml`)

All fields are optional with sensible defaults:

```yaml
# Run .test.sh inside devenv shell (default: true).
# When false, .test.sh runs directly with bash and must exist.
use_shell: true

# Initialize a git repo in the temp directory (default: true).
git_init: true

# Run in a temporary directory (default: true).
# When false, the test runs directly in its source directory.
use_tmp_dir: true

# Restrict to specific systems (empty = all systems).
supported_systems:
  - x86_64-linux
  - aarch64-darwin

# Mark systems where the test is known broken.
broken_systems:
  - aarch64-linux
```

## Execution order

For each test directory:

1. Copy test files to a temporary directory (if `use_tmp_dir: true`)
2. Run `.patch.sh` (if present) — runs outside the shell
3. Initialize git repository (if `git_init: true`)
4. Load devenv configuration
5. Run `.setup.sh` (if present) — runs inside the devenv shell
6. Run the test:
   - `use_shell: true` (default): runs `devenv test`
   - `use_shell: false`: runs `.test.sh` directly with bash
7. Report pass/fail
