---
status: deprecated
---

!!! note "Why this integration is deprecated"

    [Where `.env` Went Wrong](https://secretspec.dev/blog/where-env-went-wrong/) explains why `.env` files are a
    poor fit for configuration and secret management.

!!! danger "Dotenv values can enter the Nix store"

    Dotenv values participate in Nix evaluation and can be copied into the
    Nix store, evaluation cache, logs, derivations, or generated files. Any
    user with read access to those locations may be able to read the values.

!!! tip "Consider SecretSpec for new projects"

    For new projects, consider using [SecretSpec][secretspec] instead of `.env` files. SecretSpec provides:

    - Separation of secret declaration from provisioning
    - Support for multiple secure providers (keyring, 1Password, etc.)
    - Runtime secret loading (keeps secrets out of shell environment)
    - Better security practices and secret rotation

    See the [SecretSpec integration guide][secretspec] for more details.

[.env](https://github.com/motdotla/dotenv) files were introduced by Heroku in 2012.

If you have a `.env`, you'll see instructions how to enable integration:

```nix title="devenv.nix"
{
  dotenv.enable = true;

  # Optionally, you can choose which filename to load.
  # 
  # dotenv.filename = ".env.production";
  # or
  # dotenv.filename = [ ".env.production" ".env.development" ]

  # Expand $NAME and ${NAME:-default} references. This is off by default
  # so dollar signs in passwords, hashes, and tokens remain literal.
  # dotenv.substitution = true;

}
```

When the developer environment is loaded, the devenv CLI parses dotenv files with `dotenv-ng`.
Files later in `dotenv.filename` override values from earlier files.

Dotenv values are available through `config.env` during module evaluation. Explicit `env`
variables in `devenv.nix` have priority over values from dotenv:

```nix title="devenv.nix"
{ config, ... }:
{
  dotenv.enable = true;
  env.DATABASE_HOST_ALIAS = "db-${config.env.DATABASE_HOST}";
}
```

Because the values participate in Nix evaluation, they can be copied into the evaluation cache,
derivations, store paths, logs, or generated files. The cache tracks dotenv file hashes,
including missing layered files, and the inherited variables used by substitution.

The integration supports quoted and multiline values, comments, optional `export`
prefixes, and dotenv files in subdirectories. Relative filenames are resolved from the project
root. When substitution is enabled, values in earlier dotenv assignments take precedence over
the inherited environment. Files created by enter-shell tasks are also loaded for the final shell,
but values that did not exist during evaluation cannot be consumed by Nix modules until the next
evaluation. The devenv CLI is required; the flake integration cannot load dotenv values.

[secretspec]: secretspec.md
