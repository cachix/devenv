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
devenv-run-tests generate-json --system aarch64-darwin  # filter for a specific system
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

### The test environment

Scripts that run outside the devenv shell — `.patch.sh`, and `.test.sh` under `use_shell: false` — would otherwise depend on whatever the host happens to provide.
`test-env.nix` decides instead.
`devenv-run-tests run` embeds and builds it once per run through the Nix API, using the devenv repository's locked `nixpkgs` input. It then puts the environment's `bin` first on `PATH` for those scripts, so `curl`, `jq`, `git` and GNU coreutils are the same on every platform.
Stock macOS has no `timeout` and no `sha256sum`; here both are the GNU ones.

Add a tool by listing it in `test-env.nix`.
Set `DEVENV_TEST_ENV` to an already-built environment to skip the build.

The same file defines the shared shell helpers, and `DEVENV_TEST_LIB` points at the generated library:

```bash
. "$DEVENV_TEST_LIB"
```

The helpers call their tools by store path, so they hold even where a test rewrites `PATH`.

| Helper | Purpose |
|---|---|
| `run_bounded SECONDS CMD...` | Run a command under a failure bound |
| `wait_until SECONDS CMD...` | Retry a command until it succeeds |
| `http_is_ready [PORT]` | Whether a local HTTP port answers |
| `wait_for_http_ready PORT [seconds]` | Wait for it to start answering |
| `wait_for_http_gone [PORT] [seconds]` | Wait for it to stop answering |
| `wait_for_path_gone PATH [seconds]` | Wait for a path to disappear |
| `wait_for_pid_gone PID [seconds]` | Wait for a process to exit |
| `devenv_runtime_dir` | The runtime directory devenv derives for `$PWD` |

Every bound is in seconds, and waiting helpers poll ten times a second.

Names follow two shapes: a check is `<subject>_is_<state>`, and waiting for that same state is `wait_for_<subject>_<state>`.
Helpers that take a command take the bound first; helpers that take a subject take it last, where it is optional.
A test that needs a check of its own writes it as a predicate and passes it to `wait_until`, keeping the same shapes.

`wait_until` replaces the `timeout N bash -c 'until CMD; do sleep; done'` idiom, and takes shell functions as well as commands.

Scripts that run inside the devenv shell keep using `wait_for_port` and `wait_for_processes`, which devenv itself provides to every project.
See [tests](https://devenv.sh/tests/).

A test that needs a package for the shell itself still declares it in `devenv.nix`; the test environment covers only the scripts above.

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

# Fail the test if the closure of its shell is larger than this.
# Accepts a byte count or a number with a unit (B, KB, MB, GB, TB, KiB, MiB, GiB, TiB).
max_closure_size: 1 GB
```

## Test results

Every test reports its runtime and the closure size of the shell it built, both on the per-test `Passed`/`Failed` line and in the summary table printed at the end of the run:

```
• Test results:
  passed   man-pages                  20.4s     361 MB
  passed   postgresql-socket-only     31.2s     512 MB
  FAILED   python-packages            48.0s     1.2 GB  Shell closure is 1.2 GB, exceeding max_closure_size of 1 GB
  skipped  gpu-only                       -         -
```

The closure size is measured with `nix path-info --closure-size` on the `shell` GC root in the test's `.devenv/gc` directory, so it covers everything `devenv shell` needs to download for that test.

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
7. Measure the shell closure size and check it against `max_closure_size`
8. Report pass/fail with runtime and closure size
