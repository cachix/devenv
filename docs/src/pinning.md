# Pinning

Pinning keeps your developer environment reproducible. Each [input](inputs.md) in `devenv.yaml` is resolved to an exact revision and stored in `devenv.lock` — the same idea as a lockfile in other ecosystems (package-lock, poetry.lock, composer.lock, and so on).

## How it works

You declare inputs in `devenv.yaml`:

```yaml title="devenv.yaml"
inputs:
  nixpkgs:
    url: github:cachix/devenv-nixpkgs/rolling
```

devenv writes the resolved revisions into `devenv.lock`. After that, everyone who uses the same lockfile gets the same dependency versions — until someone runs `devenv update`.

You do not need a separate “create lock” step. The lock is created or updated when you use devenv on the project.

## Viewing pins

```shell-session
$ cat devenv.lock
```

`devenv info` also shows locked inputs.

## Pinning to a specific revision

Use `?rev=` in the input URL for an exact commit:

```yaml title="devenv.yaml"
inputs:
  nixpkgs-stable:
    url: github:NixOS/nixpkgs?rev=ac62194c3917d5f474c1a844b6fd6da2db95077d
```

Use `?ref=` for a branch or tag (for example the NixOS 25.05 release branch):

```yaml title="devenv.yaml"
inputs:
  nixpkgs-stable:
    url: github:NixOS/nixpkgs?ref=nixos-25.05
```

With `?rev=…`, that commit is fixed in the declaration. With only a branch or tag (`?ref=…` or a path like `…/nixos-25.05`), the lockfile freezes whatever commit was current when the lock was last written; it does not keep moving on every command.

## Updating pins

`devenv update` is how you **intentionally refresh** the lockfile — same role as `npm update`, `poetry update`, or similar. Day-to-day commands keep using the revisions already in `devenv.lock`. They do not pull newer commits from a floating branch just because upstream moved.

Refresh every input’s locked revision (where the URL still points at a branch or tag):

```shell-session
$ devenv update
```

Refresh a single input by name:

```shell-session
$ devenv update nixpkgs-stable
```

That re-resolves the input from its URL and rewrites `devenv.lock` if the result changed. Commit the new lock so the team and CI pick up the same pins.

## Adding inputs from the CLI

```shell-session
$ devenv inputs add nixpkgs-stable github:NixOS/nixpkgs/nixos-25.05
$ devenv inputs add my-input github:org/repo --follows nixpkgs
```

This updates `devenv.yaml`. The lock is refreshed the next time you run a devenv command on the project.

## Commit the lockfile

Commit `devenv.lock` to version control. It is what makes the environment reproducible across machines.

If the lockfile is missing, devenv creates one by resolving inputs. Without a shared lock, floating branches or tags can resolve to different commits on different machines.
