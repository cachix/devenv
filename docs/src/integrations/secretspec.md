# SecretSpec

[SecretSpec](https://secretspec.dev) separates secret declaration from secret provisioning. You define what secrets your application needs in a `secretspec.toml` file, and each developer, CI system, and production environment can provide those secrets from their preferred secure provider.

## Quick Start

Follow [SecretSpec Quick Start](https://secretspec.dev/quick-start/).

## Best Practice: Runtime Loading

While you can enable SecretSpec in devenv to load secrets into `secretspec.secrets` option, we recommend:

a) [Use Rust SDK](https://secretspec.dev/sdk/rust/)

b) Your application load secrets at runtime instead:

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

If you do need secrets in your devenv environment:

```yaml title="devenv.yaml"
secretspec:
  enable: true
  provider: keyring  # keyring, dotenv, env, 1password, lastpass
  profile: default   # profile from secretspec.toml
```

Then access in `devenv.nix`:

```nix title="devenv.nix"
{ config, ... }:

{
  env.DATABASE_URL = config.secretspec.secrets.DATABASE_URL or "";
}
```
https://secretspec.dev/sdk/rust/

## Learn More

- [secretspec.dev](https://secretspec.dev)
- [Providers](https://secretspec.dev/providers/keyring/) - Keyring, 1Password, dotenv, and more
- [Profiles](https://secretspec.dev/concepts/profiles/) - Environment-specific configurations
- [Rust SDK](https://secretspec.dev/sdk/rust/) - Type-safe 
