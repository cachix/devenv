# Profile Tests

This directory contains tests for the profiles functionality implemented in issue #2137.

## Test Structure

- `devenv.nix` - Basic profile definitions for testing
- `test.sh` - Basic profile functionality tests  
- `profile-merging/` - Tests for profile merging and precedence
- `cli-integration/` - Tests for CLI --profile option integration
- `run-all-tests.sh` - Script to run all profile tests

## Running Tests

To run all profile tests:
```bash
./run-all-tests.sh
```

To run individual tests:
```bash
./test.sh                              # Basic functionality
./profile-merging/test.sh              # Merging and precedence  
./cli-integration/test.sh              # CLI integration
```

## Profile Features Tested

1. **Basic Profile Definition**: Profiles defined with `profiles.<name>.config = { ... }`
2. **Single Profile Activation**: `devenv --profile <name> <command>`
3. **Multiple Profile Activation**: `devenv --profile <name1> --profile <name2> <command>`
4. **Profile Precedence**: Later profiles override earlier ones
5. **Base Configuration Merging**: Profiles merge with base configuration
6. **CLI Integration**: Both `-P` and `--profile` flags work
7. **Error Handling**: Invalid profile names are handled gracefully

## Example Usage

```nix
# In devenv.nix
{ pkgs, config, lib, ... }: {
  # Base configuration
  languages.python = {
    enable = true;
    version = "3.15";
  };

  # Profile definitions
  profiles."python-3.14".config = {
    languages.python.version = "3.14";
  };

  profiles."backend".config = {
    services.postgres.enable = true;
    services.redis.enable = true;
  };
}
```

```bash
# Use profiles
devenv --profile python-3.14 shell
devenv --profile backend --profile python-3.14 up
```