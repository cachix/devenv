# devenv-run-tests

A test runner for devenv that executes integration tests in isolated environments.

## Overview

`devenv-run-tests` runs integration tests by:
1. Creating temporary directories for each test
2. Copying test files to the temporary directory
3. Setting up the devenv environment
4. Running test scripts in the devenv shell
5. Reporting results

## Usage

```bash
# Run all tests in default directories (examples/ and tests/)
devenv-run-tests

# Run tests in specific directories
devenv-run-tests path/to/tests another/path

# Run only specific tests
devenv-run-tests --only test1 --only test2

# Exclude specific tests
devenv-run-tests --exclude flaky-test --exclude slow-test

# Override inputs in devenv.yaml
devenv-run-tests --override-input nixpkgs github:NixOS/nixpkgs/nixos-unstable
```

## Test Structure

Each test is a directory containing:
- `devenv.nix` - The devenv configuration
- `devenv.yaml` - Input specifications (optional)
- Additional test files and scripts

### Test Execution Files

#### `.test.sh` (Optional)
An optional test script that defines custom test logic. This script runs inside the devenv shell. If not present, the test runner will use `devenv test` which executes the `enterTest` defined in your `devenv.nix`.

```bash
#!/usr/bin/env bash
set -ex

# Your test logic here
echo "Running test..."
some-command
```

#### `.setup.sh` (Optional)
A setup script that runs inside the devenv shell before the test. Use this to prepare the environment or install dependencies.

```bash
#!/usr/bin/env bash
set -ex

# Setup logic here
npm install
createdb myapp
```

#### `.patch.sh` (Optional)
A patch script that runs in the working directory before the devenv shell is created. Use this to modify files before devenv evaluation.

```bash
#!/usr/bin/env bash
set -ex

# Patch files before devenv starts
echo 'additional-config' >> devenv.nix
sed -i 's/old-value/new-value/' some-file.txt
```

#### `.test-config.yml` or `.test-config.yaml` (Optional)
A YAML configuration file that controls test behavior.

```yaml
# Whether to initialize a git repository for the test
# Default: true
git_init: false
```

## Test Configuration

### Git Repository Behavior

By default, each test runs in a temporary directory with a fresh git repository. This helps:
- Nix Flakes find the project root
- git-hooks tests work correctly
- Tests run in isolation

To disable git repository creation for a test, create a `.test-config.yml` file:

```yaml
git_init: false
```

This is useful for:
- Testing behavior outside of git repositories
- Testing flake evaluation without git context
- Debugging caching issues that only occur in non-git environments

## Execution Order

For each test directory, devenv-run-tests:

1. **Copies test files** to a temporary directory
2. **Changes to the temporary directory**
3. **Runs `.patch.sh`** (if present) in the working directory
4. **Initializes git repository** (if `git_init: true` in config)
5. **Sets up devenv environment**
6. **Runs `.setup.sh`** (if present) inside the devenv shell
7. **Runs test**: Either `.test.sh` (if present) or `devenv test` (which executes `enterTest` from `devenv.nix`)
8. **Reports test results**

## Examples

### Basic Test (using enterTest)
```
tests/my-test/
├── devenv.nix     # Contains enterTest definition
└── devenv.yaml
```

### Basic Test (using custom script)
```
tests/my-test/
├── devenv.nix
├── devenv.yaml
└── .test.sh       # Custom test script
```

### Test with Setup
```
tests/database-test/
├── devenv.nix
├── devenv.yaml
├── .setup.sh      # Initialize database
└── .test.sh       # Run database tests
```

### Test without Git
```
tests/no-git-test/
├── devenv.nix
├── devenv.yaml
├── .test-config.yml   # git_init: false
└── .test.sh
```

### Test with Patching
```
tests/patch-test/
├── devenv.nix
├── devenv.yaml
├── .patch.sh      # Modify files before devenv
└── .test.sh
```

## Environment Variables

- `DEVENV_NIX` - Path to the custom Nix build (required)
- `DEVENV_RUN_TESTS` - Internal flag to control test execution

## Exit Codes

- `0` - All tests passed
- `1` - One or more tests failed

## Output

The test runner provides:
- Progress indicators for each test
- Summary of passed/failed tests
- Details about failed tests

```
Running Tests
Running in directory tests
  Running my-test
  Running database-test
  Running no-git-test

my-test: Failed

Ran 3 tests, 1 failed.
```