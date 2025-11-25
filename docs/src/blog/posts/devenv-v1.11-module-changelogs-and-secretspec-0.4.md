---
date: 2025-11-26
authors:
  - domenkozar
draft: false
---

# devenv 1.11: Module changelogs and SecretSpec 0.4.0

[devenv 1.11](https://github.com/cachix/devenv/releases/tag/v1.11) brings two key improvements:

- [Module changelogs](#module-changelogs) for communicating breaking changes
- [Profile configuration in devenv.yaml](#profile-configuration-in-devenvyaml)
- [SecretSpec 0.4.0](#secretspec-040) with multiple provider support and file-based secrets

## Module changelogs

The Nix module system already handles renames and deprecations well—you get clear warnings when using old option names. But communicating *behavior* changes is harder. When a default value changes or a feature works differently, users often discover this through unexpected behavior rather than explicit notification.

Recently we've wanted to [change `git-hooks.package`](https://github.com/cachix/devenv/pull/2304) from `pkgs.pre-commit` to `pkgs.prek`, a reimplementation in Rust.

The new changelog option lets module authors declare important changes directly in their modules:

```nix title="devenv.nix"
{ config, ... }: {
  changelogs = [
    {
      date = "2025-11-26";
      title = "git-hooks.package now defaults to pkgs.prek";
      when = config.git-hooks.enable;
      description = ''
        The git-hooks integration now uses [prek](https://github.com/cachix/prek) by default for speed and smaller binary size.

        If you were using pre-commit hooks, update your configuration:
        ```nix
        git-hooks.package = pkgs.pre-commit;
        ```
      '';
    }
  ];
}
```

Each entry includes:

- `date`: When the change was introduced (YYYY-MM-DD)
- `title`: Short summary of what changed
- `when`: Condition for showing this changelog (show only to affected users)
- `description`: Markdown-formatted details and migration steps

After running `devenv update`, relevant new changelogs are displayed automatically:

```shell-session
$ devenv update
...

📋 changelog

2025-11-24: **git-hooks.package now defaults to pkgs.prek**

  The git-hooks integration now uses prek by default.

  If you were using pre-commit hooks, update your configuration:
    git-hooks.package = pkgs.pre-commit;
```

The `when` condition ensures changelogs only appear to users who have the relevant feature enabled. A breaking change to PostgreSQL configuration won't bother users who don't use PostgreSQL.

View all relevant changelogs anytime with:

```shell-session
$ devenv changelogs
```

### For module authors

If you maintain devenv modules (either in-tree or as external imports), add changelog entries when making breaking changes. This helps your users stay informed without requiring them to read through commit history or release notes.

See the [contributing guide](../../community/contributing.md#adding-changelogs-for-breaking-and-behavior-changes) for details.

## Profile configuration in devenv.yaml

You can now specify the default profile in `devenv.yaml` or `devenv.local.yaml`:

```yaml title="devenv.yaml"
profile: fullstack
```

This can be overridden with the `--profile` CLI flag.

## SecretSpec 0.4.0

We've released [SecretSpec 0.4.0](https://secretspec.dev) with two major features: multiple provider support and file-based secrets.

### Multiple providers with fallback chains

You can now configure different providers for individual secrets, with automatic fallback:

```toml title="secretspec.toml"
[profiles.production]
DATABASE_URL = { description = "Production DB", providers = ["prod_vault", "keyring"] }
API_KEY = { description = "API key", providers = ["env"] }
```

Define provider aliases in your user config:

```shell-session
$ secretspec providers add prod_vault onepassword://vault/Production
$ secretspec providers add shared_vault onepassword://vault/Shared
```

When multiple providers are specified, SecretSpec tries each in order until it finds the secret. This enables:

- **Shared vs local**: Try a team vault first, fall back to local keyring
- **Migration**: Gradually move secrets between providers
- **Multi-source setups**: Projects that need to source secrets from different providers

Combine that with profile-level defaults to avoid repetition:

```toml
[profiles.production.defaults]
providers = ["prod_vault", "keyring"]
required = true

[profiles.production]
DATABASE_URL = { description = "Production DB" }  # Uses default providers
API_KEY = { description = "API key", providers = ["env"] }  # Override
```

### Provisioning secrets as a file

Some tools require secrets as file paths rather than values—certificates, SSH keys, service account credentials. 

```toml
[profiles.default]
TLS_CERT = { description = "TLS certificate", as_path = true }
```

With `as_path = true`, SecretSpec writes the secret value to a secure temporary file and returns the path instead:

```shell-session
$ secretspec get TLS_CERT
/tmp/secretspec-abc123/TLS_CERT
```

In Nix, we don't want to leak secrets into the world-readable store, so passing them as paths avoids this issue:

```nix title="devenv.nix"
{ pkgs, config, ... }: {
  services.myservices.certPath = config.secretspec.secrets.TLS_CERT;
}
```

Temporary files are automatically cleaned up when the resolved secrets are dropped.

If you haven't tried SecretSpec yet, see [Announcing SecretSpec](announcing-secretspecs-declarative-secrets-management.md) for an introduction.

## Getting started

New to devenv? Check out the [getting started guide](../../getting-started.md).

Join the [devenv Discord community](https://discord.gg/naMgvexb6q) to share feedback!

Domen
