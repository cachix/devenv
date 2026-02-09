# SecretSpec

[SecretSpec] separates secret declaration from secret provisioning.
You define the secrets that your application needs in a `secretspec.toml` file and each developer, CI system, and production environment can provide those secrets from their preferred secure provider.

## Quick Start

Follow the [SecretSpec quickstart guide][SecretSpec Quick Start].

## Runtime Loading (Best Practice)

While you can enable SecretSpec in devenv to load secrets into the `secretspec.secrets` option, we recommend that you:

a) [Use the Rust SDK][Rust SDK] to load secrets in your application code

b) Load secrets at runtime and expose them only to the processes that need them

```bash
$ devenv shell
$ secretspec run -- npm start
```

This approach:

- Keeps secrets out of your shell environment
- Reduces exposure of sensitive data
- Makes secret rotation easier
- Follows the principle of least privilege

## Configuration (Optional)

If you do need secrets in your devenv environment, you can configure via `devenv.yaml` or CLI flags.

### Via CLI flags (devenv 2.0+)

Override the provider and profile directly from the command line:

```bash
$ devenv --secretspec-provider dotenv --secretspec-profile dev shell
```

This automatically enables secretspec. You can also use environment variables:

```bash
$ SECRETSPEC_PROVIDER=dotenv SECRETSPEC_PROFILE=dev devenv shell
```

### Via devenv.yaml

```yaml title="devenv.yaml"
secretspec:
  enable: true
  provider: keyring  # keyring, dotenv, env, 1password, lastpass
  profile: default   # profile from secretspec.toml
```

CLI flags take precedence over `devenv.yaml` values.

Then access in `devenv.nix`:

```nix title="devenv.nix"
{ config, ... }:

{
  env.DATABASE_URL = config.secretspec.secrets.DATABASE_URL or "";
}
```

## Learn More

- [SecretSpec]
- [Providers] - Keyring, 1Password, dotenv, and more
- [Profiles] - Environment-specific configurations
- [Rust SDK] - Type-safe

[SecretSpec]: https://secretspec.dev
[SecretSpec Quick Start]: https://secretspec.dev/quick-start/
[Rust SDK]: https://secretspec.dev/sdk/rust/
[Providers]: https://secretspec.dev/providers/keyring/
[Profiles]: https://secretspec.dev/concepts/profiles/
