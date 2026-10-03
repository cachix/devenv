---
title: Sandboxing with nono
---

Enable [nono](https://nono.sh) sandboxing in `devenv.yaml`:

```yaml
sandbox: true
```

The default is `false`. devenv builds nono from your project's nixpkgs input, so no separate installation is needed. Your nixpkgs must provide the `nono` package.

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
| Network | Enabled |
| Inherited environment variables, including secrets | Available to commands |

nono also applies its baseline system permissions, including writes to standard temporary directories even when `$TMPDIR` points elsewhere. Temporary directories are shared with other host applications.

Inherited `NONO_*` variables are cleared before nono starts so they cannot change this policy.

## Home directories and direnv

Tools that write caches or configuration under your home directory may need to use project-local paths instead. Shell startup files under your home directory are also subject to the sandbox. direnv exports variables into your existing shell; it does not sandbox that shell.

## Disabling sandboxing locally

To override `sandbox: true` for your checkout, add this to `devenv.local.yaml`:

```yaml
sandbox: false
```
