# Garbage collection

devenv creates garbage collection (GC) roots so that Nix does not delete your developer environments from the store while they are still in use.

## How it works

Each time you activate a shell (or run another devenv command that evaluates your environment), devenv creates a timestamped symlink inside `$DEVENV_HOME/gc/` (typically `~/.local/share/devenv/gc/`). The symlink points to the Nix store path that backs the environment.

When you run `devenv gc`:

1. **Dangling symlinks are removed.** Any symlink in the GC directory whose target no longer exists is deleted.
2. **Live store paths are collected.** The remaining symlinks are resolved to their Nix store paths.
3. **Nix garbage collection runs.** The resolved paths are passed to Nix GC, which deletes any store path not reachable from a live root.

Only the latest successful invocation per project folder is kept; older generations are cleaned up automatically.

## Usage

```shell-session
$ devenv gc
```

Run this whenever you want to reclaim disk space. It is safe to run at any time since it only removes store paths that are no longer referenced by any active environment.
