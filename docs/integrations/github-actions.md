---
title: Using devenv in GitHub Actions
description: Use developer environments powered by devenv to check, build, and test code in GitHub Actions workflows.
---

# GitHub Actions

### Introduction

[GitHub Actions][github-actions] is a continuous integration (CI) platform built into GitHub.

Devenv allows you to reuse your existing development environment in your [GitHub Actions][github-actions] workflows to run checks, builds, tests, and more.

This guide will go through the steps required to set up devenv in a [GitHub Actions][github-actions] workflow and show you how to run commands in the devenv shell.
We'll use the following sample devenv configuration in our examples.

```nix title="devenv.nix"
{ pkgs, ... }:

{
  packages = [ pkgs.hello ];

  scripts.say-bye.exec = ''
    echo bye
  '';
}
```

The `hello` package is a program that prints "Hello, world!" and the custom `say-bye` script prints "bye".

A [complete workflow example](#complete-example) is available at the end of this guide.

### Prerequisites

Let's first prepare the job environment for devenv.

```yaml
steps:
- uses: actions/checkout@v4
- uses: cachix/install-nix-action@v23
- uses: cachix/cachix-action@v12
  with:
    name: devenv
- name: Install devenv.sh
  run: nix profile install tarball+https://install.devenv.sh/latest
```

The above snippet does the following:

1. Checks out the repository.
2. Installs and sets up [Nix][nix].
3. Configures [Nix][nix] to use the devenv cache provided by [Cachix][cachix] to speed up the installation.
4. Installs devenv.

If you're using a [self-hosted runner](https://docs.github.com/en/actions/hosting-your-own-runners/managing-self-hosted-runners/about-self-hosted-runners),
you can pre-install both Nix and devenv, and skip the associated steps.

### `devenv test`

Devenv provides a convenient built-in `devenv test` command.
It builds the shell and runs any defined [pre-commit hooks](../pre-commit-hooks.md) against your repository.
This is a quick and easy way to test that your development environment works as expected and lint your code at the same time.

```yaml
- name: Build the devenv shell and run any pre-commit hooks
  run: devenv test
```

### Run a single command

Single commands can be passed to `devenv shell` to be run in the devenv shell.

```yaml
- name: Run a single command in the devenv shell
  run: devenv shell hello
```

```console title="Output"
Building shell ...
Hello, world!
```

### Run multiple commands

Each `run` step in a job launches a separate shell.
That's why we can't just run `devenv shell` in one `run` step and have all subsequent commands run in the same devenv shell.

Instead, we can use the [`shell` option](https://docs.github.com/en/actions/using-workflows/workflow-syntax-for-github-actions#jobsjob_idstepsshell)
to override the default shell for the current step and replace it with the devenv shell.

```yaml
- name: Run a multi-line command in the devenv shell
  shell: devenv shell bash -e {0}
  run: |
    hello
    say-bye
```
```console title="Output"
Building shell ...
Hello, world!
bye
```

Overriding the shell can become quite tedious when you have a lot of separate `run` steps.
You can use the [`defaults.run`](https://docs.github.com/en/actions/using-workflows/workflow-syntax-for-github-actions#defaultsrun)
option to set devenv as the default shell for all `run` steps in a job.

```yaml
defaults:
  run:
    shell: devenv shell bash -e {0}
```

### Complete Example

Let's put all of the above together in a complete example workflow.

```yaml title=".github/workflows/test.yml"
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
    - uses: actions/checkout@v4
    - uses: cachix/install-nix-action@v23
    - uses: cachix/cachix-action@v12
      with:
        name: devenv
    - name: Install devenv.sh
      run: nix profile install --accept-flake-config tarball+https://install.devenv.sh/latest

    - name: Build the devenv shell and run any pre-commit hooks
      run: devenv test

    - name: Run a single command in the devenv shell
      run: devenv shell hello

    - name: Run a multi-line command in the devenv shell
      shell: devenv shell bash -e {0}
      run: |
        hello
        say-bye
```

[github-actions]: https://docs.github.com/actions
[cachix]: https://cachix.org
[nix]: https://nixos.org
