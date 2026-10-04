---
title: Sandboxing with nono
---

Enable [nono](https://nono.sh) sandboxing in `devenv.yaml`:

```yaml
sandbox:
  enable: true
```

`sandbox.enable` defaults to `false`; `sandbox.networking.enable` defaults to `true`. devenv builds nono from your project's nixpkgs input, so no separate installation is needed. Your nixpkgs must provide the `nono` package.

nono uses Landlock on Linux and Seatbelt on macOS. If nono cannot apply the sandbox, the command fails.

## Running sandboxed commands

Once enabled, use the usual devenv commands:

```shell
# Enter an interactive shell, with reload support.
devenv shell

# Run a command in the environment.
devenv shell -- bash -c 'echo hello > result.txt'

# Run a configured task or service.
devenv tasks run my-task
devenv up my-service
```

The sandbox applies to interactive shells (including shells with reload), `devenv shell -- <command>`, tasks and their status checks, and services launched by devenv. Exec readiness probes for native services also run inside the sandbox. Nix evaluation and builds run outside this runtime sandbox.

## Permissions

| Resource | Access |
| --- | --- |
| Project directory | Read and write |
| devenv state and runtime directories | Read and write |
| `$TMPDIR` (default `/tmp`) | Read and write |
| Nix store and system tooling directories | Read |
| Other paths, including home directories | Restricted by nono |
| Network | Enabled by default; configurable with `sandbox.networking.enable` |
| Inherited environment variables, including secrets | Available to commands |

nono also applies its baseline system permissions, including writes to standard temporary directories even when `$TMPDIR` points elsewhere. Temporary directories are shared with other host applications.

The launcher suppresses inherited Bash startup files and functions until nono applies the sandbox. Normal shell startup then runs within the sandbox. Inherited `NONO_*` variables are cleared before nono starts so they cannot change this policy.

## Additional paths

Configure additional permissions under `sandbox` in `devenv.yaml`:

```yaml
sandbox:
  enable: true
  read:
    - ../shared-data
    - ~/.gitconfig
  write:
    - ~/.cache/my-tool
```

Both lists accept existing files and directories. `read` grants read-only access; `write` grants read and write access. Relative project paths resolve from the project root, including when devenv is invoked from a subdirectory. `~` and `~/` expand to your home directory. Paths are canonicalized, so symlinks grant access to their targets. Missing paths are errors; create cache directories before configuring them.

For personal permissions shared by all your sandboxed projects, add the same lists to your [user configuration](/tui-customization/):

```yaml
# ~/.config/devenv/config.yaml
version: 1
sandbox:
  read:
    - ~/.gitconfig
  write:
    - ~/.cache/my-tool
```

The default user configuration path is `$XDG_CONFIG_HOME/devenv/config.yaml`, falling back to `~/.config/devenv/config.yaml`. Use `devenv --user-config /path/to/config.yaml ...` to select another file. Relative paths in these lists resolve from the directory containing that user config file.

Project and user lists are combined. Project lists also accumulate across imports and `devenv.local.yaml`. Duplicate paths are removed, and read/write access takes precedence when the same path appears in both lists. Directory grants include their children, so a read-only entry cannot narrow a broader read/write grant.

User permissions apply to every sandboxed command, including noninteractive commands and tasks. The user config can also set `sandbox.enable` and `sandbox.networking.enable` as personal defaults. Explicit project settings override these defaults, and `devenv.local.yaml` overrides project settings. When `sandbox.enable: false`, the additional path permissions are ignored. The inherited `NONO_*` variables remain ignored.

## Networking

Network access is enabled by default. To block IP networking for shells, tasks, and services:

```yaml
sandbox:
  enable: true
  networking:
    enable: false
```

The same `sandbox.networking.enable` setting can be added to your user config as a default. An explicit project setting takes precedence. Set it to `true` to allow networking again. Networking settings have no effect when sandboxing is disabled.

This maps to nono's `--block-net`: IP networking is blocked, including localhost TCP and UDP. Local Unix-domain sockets remain subject to nono's socket permissions. On Linux, nono also blocks `io_uring_setup`, so programs using io_uring for file I/O need to disable that feature. Nix evaluation and builds continue outside the sandbox and retain their network access.

## Home directories and direnv

Tools that write caches or configuration under your home directory can use project-local paths or an explicit `sandbox.write` grant. Shell startup files under your home directory are also subject to the sandbox. direnv exports variables into your existing shell; it does not sandbox that shell.

## Disabling sandboxing locally

To override `sandbox.enable: true` for your checkout, add this to `devenv.local.yaml`:

```yaml
sandbox:
  enable: false
```
