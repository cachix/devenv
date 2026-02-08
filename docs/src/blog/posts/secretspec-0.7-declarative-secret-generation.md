---
date: 2026-02-09
authors:
  - domenkozar
draft: false
---

# SecretSpec 0.7: Declarative Secret Generation

If you haven't tried SecretSpec yet, see [Announcing SecretSpec](announcing-secretspecs-declarative-secrets-management.md) for an introduction.

SecretSpec 0.7 introduces **declarative secret generation** — declare that secrets should be auto-generated when missing, directly in your `secretspec.toml`.

## The Problem

When onboarding to a project, developers typically need to:

1. Read docs to understand which secrets are needed
2. Manually generate passwords and tokens
3. Store them in the right provider

Some secrets — like local database passwords or session keys — don't need to be shared at all. They just need to exist.

## The Solution: `type` + `generate`

Add `type` and `generate` to any secret declaration, and SecretSpec handles the rest:

```toml
[project]
name = "my-app"
revision = "1.0"

[profiles.default]
DB_PASSWORD = { description = "Database password", type = "password", generate = true }
API_TOKEN = { description = "Internal API token", type = "hex", generate = { bytes = 32 } }
SESSION_KEY = { description = "Session signing key", type = "base64", generate = { bytes = 64 } }
REQUEST_ID = { description = "Request ID prefix", type = "uuid", generate = true }
```

Run `secretspec check` or `secretspec run`, and any missing secret with `generate` configured is automatically created and stored in your provider:

```
$ secretspec check
Checking secrets in my-app (profile: default)...

✓ DB_PASSWORD - generated and saved to keyring (profile: default)
✓ API_TOKEN - generated and saved to keyring (profile: default)
✓ SESSION_KEY - generated and saved to keyring (profile: default)
✓ REQUEST_ID - generated and saved to keyring (profile: default)

Summary: 4 found, 0 missing
```

On subsequent runs, the stored values are reused — generation is idempotent.

## Five Generation Types

| Type | Default Output | Options |
|------|---------------|---------|
| `password` | 32 alphanumeric characters | `length`, `charset` (`"alphanumeric"` or `"ascii"`) |
| `hex` | 64 hex characters (32 bytes) | `bytes` |
| `base64` | 44 characters (32 bytes) | `bytes` |
| `uuid` | UUID v4 | none |
| `command` | stdout of a shell command | `command` (required) |

### Custom Options

Use a table instead of `true` for fine-grained control:

```toml
# 64-character password with printable ASCII
ADMIN_PASSWORD = { description = "Admin password", type = "password", generate = { length = 64, charset = "ascii" } }

# 64 random bytes, hex-encoded (128 chars)
ENCRYPTION_KEY = { description = "Encryption key", type = "hex", generate = { bytes = 64 } }
```

### Shell Commands

The `command` type runs arbitrary shell commands, covering any generation need:

```toml
# WireGuard private key
WG_PRIVATE_KEY = { description = "WireGuard key", type = "command", generate = { command = "wg genkey" } }

# MongoDB keyfile
MONGO_KEYFILE = { description = "MongoDB keyfile", type = "command", generate = { command = "openssl rand -base64 765" } }

# SSH public key (from existing key)
SSH_PUBKEY = { description = "SSH public key", type = "command", generate = { command = "ssh-keygen -y -f ~/.ssh/id_ed25519" } }
```

## Design Decisions

**Generate if missing, never overwrite.** Existing secrets are always preserved. This makes generation safe to declare in shared config files — it only fills in gaps.

**No separate `generate` command.** Generation happens automatically during `check` and `run`. A dedicated CLI command for rotation is planned for a future release.

**`type` without `generate` is valid.** You can annotate secrets with a type for documentation purposes without enabling generation. This is useful for secrets that must be manually provisioned but benefit from type metadata.

**Conflicts are caught early.** `generate` + `default` on the same secret is an error (which value should win?). `type = "command"` with `generate = true` (no command string) is also an error.

## Upgrading

Update to SecretSpec 0.7 and add `type`/`generate` to any secrets you want auto-generated. Existing configurations continue to work without changes — both fields are optional.

```bash
curl -sSL https://install.secretspec.dev | sh
```

See the [configuration reference](https://secretspec.dev/reference/configuration/#secret-generation) for full documentation.

Share your thoughts on our [Discord community](https://discord.gg/naMgvexb6q) or [open an issue on GitHub](https://github.com/cachix/secretspec/issues).

Domen
