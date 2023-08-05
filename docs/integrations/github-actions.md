# GitHub Actions

If you'd like to run `devenv` as a workflow, create `.github/workflows/test.yml`:

```yaml
name: "Test"

on:
  pull_request:
  push:

jobs:
  tests:
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest]
    runs-on: {{ '${{ matrix.os }}' }}
    steps:
    - uses: actions/checkout@v3
    - uses: cachix/install-nix-action@v22
    - uses: cachix/cachix-action@v12
      with:
        name: devenv
    - name: Install devenv.sh
      run: nix profile install tarball+https://install.devenv.sh/latest
      shell: sh
    - run: devenv ci
    - run: devenv shell echo ok
```
