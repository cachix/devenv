---
date: 2025-07-22
authors:
  - domenkozar
draft: false
---

# devenv 1.8: Progress TUI, SecretSpec Integration, Listing Tasks, and Smaller Containers

[devenv 1.8](https://github.com/cachix/devenv/releases/tag/v1.8) fixes a couple of ~~annoying~~ regressions since the 1.7 release, but also includes several new features:

- [Progress TUI](#progress-tui) with async core
- [SecretSpec integration](#secretspec-integration) for declarative secrets management
- [Task improvements](#task-improvements) with task listing
- [CLI improvements](#cli-improvements) with package options support
- [Smaller containers](#container-optimizations) with 67% smaller images

## Progress TUI

We've rewritten our [tracing integration](https://github.com/cachix/devenv/pull/1969) to improve reporting on what devenv is doing.

More importantly, devenv is now [fully asynchronous under the hood](https://github.com/cachix/devenv/pull/1970), enabling parallel execution of operations. This means faster performance in scenarios where multiple independent tasks can run simultaneously.

The new progress interface provides real-time feedback on what devenv is doing:

![devenv progress bar](../../assets/images/devenv-progress-bar.gif)

We're continuing to improve visibility into Nix operations to give you even better insights into the build process.

## SecretSpec Integration

We've integrated [SecretSpec](https://secretspec.dev), a new standard for declarative secrets management that separates secret declaration from provisioning.

This allows teams to define what secrets applications need while letting each developer, CI system, and production environment provide them from their preferred secure provider.

Learn more in [Announcing SecretSpec Declarative Secrets Management](announcing-secretspecs-declarative-secrets-management.md).

## Task improvements

### Listing tasks

The `devenv tasks list` command now groups tasks by namespace, providing a cleaner and more organized view:

```shell-session
$ devenv tasks list
backend:
  └── lint (has status check)
      └── test
          └── build (watches: src/backend/**/*.py)
deploy:
  └── production
docs:
  └── generate (watches: docs/**/*.md)
      └── publish
frontend:
  └── lint
      └── test (has status check)
          └── build
```

### Running multi-level tasks

You can now run tasks at any level in the hierarchy. By default, tasks run in single mode (only the specified task):

```bash
# Run only frontend:build (default single mode)
$ devenv tasks run frontend:build
Running tasks     frontend:build
Succeeded         frontend:build                           5ms
1 Succeeded                         5.75ms

# Run frontend:build with all its dependencies (before mode)
$ devenv tasks run frontend:build --mode before
Running tasks     frontend:build
Succeeded         frontend:lint                            4ms
Succeeded         frontend:test                            10ms
Succeeded         frontend:build                           4ms
3 Succeeded                         20.36ms

# Run frontend:build and all tasks that depend on it (after mode)
$ devenv tasks run frontend:build --mode after
Running tasks     frontend:build
Succeeded         frontend:build                           5ms
Succeeded         deploy:production                        5ms
2 Succeeded                         11.44ms
```

## CLI improvements

### Package options support

The CLI now supports specifying single packages via the `--option` flag ([#1988](https://github.com/cachix/devenv/pull/1988)). This allows for more flexible package configuration directly from the command line:

```shell-session
$ devenv shell --option "languages.java.jdk.package:pkg" "graalvm-oracle"
```

## Container optimizations

The CI container [ghcr.io/cachix/devenv/devenv:v1.8](https://ghcr.io/cachix/devenv/devenv:v1.8) has been reduced (uncompressed) from 1,278 MB in v1.7 to 414 MB in v1.8—that's a reduction of over 860 MB (67% smaller!).

This makes [devenv container](../../integrations/devenv-container.md) much faster to pull and more efficient in CI/CD pipelines.

## Thank You

Join our [Discord community](https://discord.gg/naMgQehY) to share your experiences and help shape devenv's future!

Domen
