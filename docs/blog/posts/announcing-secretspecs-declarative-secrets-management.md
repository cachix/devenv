---
date: 2025-07-11
authors:
  - domenkozar
draft: false
---

# Announcing SecretSpec: Declarative Secrets Management

We've supported [.env integration](https://devenv.sh/integrations/dotenv/) for managing secrets, but it has several issues:

- **Apps are disconnected from their secrets** - applications lack a clear contract about which secrets they need
- **Parsing `.env` is unclear** - comments, multiline values, and special characters all have ambiguous behavior across different parsers
- **Password manager integration is difficult** - requiring manual copy-paste or template workarounds
- **Vendor lock-in** - applications use custom parsing logic, making it hard to switch providers
- **No encryption** - `.env` files are stored as plain text, vulnerable to accidental commits or unauthorized access

While we could recommend solutions like [dotenvx](https://dotenvx.com/) to encrypt `.env` files or [sops](https://github.com/getsops/sops) for general secret encryption, these bring new challenges:

<blockquote class="twitter-tweet" align="center"><p lang="en" dir="ltr">Don&#39;t you feel some anxiety given we&#39;ve normalized committing encrypted secrets to git repos?</p>&mdash; Domen Kožar (@domenkozar) <a href="https://twitter.com/domenkozar/status/1946244199663161712?ref_src=twsrc%5Etfw">July 18, 2025</a></blockquote> <script async src="https://platform.twitter.com/widgets.js" charset="utf-8"></script>

- **Single key management** - requires distributing and managing a master key
- **Trust requirements** - everyone with the key can decrypt all secrets
- **Rotation complexity** - departing team members require key rotation and re-encrypting all secrets

Larger teams often adopt solutions like [OpenBao](https://openbao.org/) (the open source fork of HashiCorp Vault), requiring significant infrastructure and operational overhead. Smaller teams face a gap between simple `.env` files and complex enterprise solutions.

What if instead of choosing one tool, we declared secrets uniformly and let each environment use its best provider?

## The Hidden Problem: Conflating Three Concerns

We've created [SecretSpec](https://secretspec.dev) and integrated it into devenv. SecretSpec separates secret management into three distinct concerns:

- **WHAT** - Which secrets does your application need? (DATABASE_URL, API_KEY)
- **HOW** - Requirements (required vs optional, defaults, validation, environment)
- **WHERE** - Where are these secrets stored? (environment variables, Vault, AWS Secrets Manager)

By separating these concerns, your [application declares what secrets it needs](https://secretspec.dev/concepts/declarative/) in a simple TOML file. Each [developer, CI system, and production environment](https://secretspec.dev/concepts/profiles/) can provide those secrets from their [preferred secure storage](https://secretspec.dev/concepts/providers/) - **without changing any application code**.

## One Spec, Multiple Environments, Different Providers

Imagine you commit a `secretspec.toml` file that declares:

```toml
# secretspec.toml - committed to your repo
[project]
name = "my-app"
revision = "1.0"

[profiles.default]
DATABASE_URL = { description = "PostgreSQL connection string", required = true }
REDIS_URL = { description = "Redis connection string", required = false }
STRIPE_API_KEY = { description = "Stripe API key", required = true }

[profiles.development]
# Inherits from default profile - only override what changes
DATABASE_URL = { default = "postgresql://localhost/myapp_dev" }
REDIS_URL = { default = "redis://localhost:6379" }
STRIPE_API_KEY = { description = "Stripe API key (test mode)" }

[profiles.production]
# Production keeps strict requirements from default profile
```
Now, here's the magic:

- **You** (on macOS): Store it in Keychain, retrieve with `secretspec --provider keyring run -- cmd args`
- **Your teammate** (on Linux): Store it in GNOME Keyring, same command works
- **That one developer**: Still uses a `.env` file locally (we don't judge, we've been there)
- **CI/CD**: Reads from environment variables in GitHub Actions `secretspec --provider env run -- cmd args`
- **Production**: Secrets get provisioned using AWS Secret Manager

Same specification. Different providers. Zero code changes.

## Example: One Spec, Three Environments

Let's walk through migrating from `.env` to SecretSpec.

### Setting up secretspec for development

First, choose your default provider and profile:

```shell-session
$ secretspec config init
? Select your preferred provider backend:
> keyring: Uses system keychain (Recommended)
  onepassword: OnePassword password manager
  dotenv: Traditional .env files
  env: Read-only environment variables
  lastpass: LastPass password manager
? Select your default profile:
> development
  default
  none
✓ Configuration saved to ~/.config/secretspec/config.toml
```

### Importing secrets

Create `secretspec.toml` from your existing `.env`:

```shell-session
$ secretspec init --from dotenv
```

### 1. Local Development with devenv (You're on macOS)

Enable SecretSpec in `devenv.yaml`:

```yaml
secretspec:
  enable: true
```

In `devenv.nix`:

```nix
{ pkgs, lib, config, ... }:

{
  services.minio = {
    enable = true;
    buckets = [ config.secretspec.secrets.BUCKET_NAME ];
  };
}
```

Start the minio process:

```shell-session
$ devenv up
✓ Starting minio...
```

### 2. CI/CD (GitHub Actions)
```yaml
# .github/workflows/test.yml
- name: Run tests
  env:
    DATABASE_URL: {{ secrets.TEST_DATABASE_URL }}
    STRIPE_API_KEY: {{ secrets.STRIPE_TEST_KEY }}
  run: |
    secretspec run --provider env --profile production -- npm test
```

### 3. Production (Fly.io)
```toml
# fly.toml
[processes]
web = "secretspec run --provider env --profile production -- npm start"

# Set secrets using fly CLI:
# fly secrets set DATABASE_URL=postgresql://... STRIPE_API_KEY=sk_live_...
# SecretSpec will read these from environment variables
```

**Notice what didn't change?** Your `secretspec.toml`. Same specification, different providers, zero code changes.


## Loading secrets in your application

While `secretspec run` provides secrets as environment variables, your application remains disconnected from knowing which secrets it requires. The Rust SDK bridges this gap by providing type-safe access to your declared secrets.

The Rust SDK provides compile-time guarantees:

```rust
// Generate typed structs from secretspec.toml
secretspec_derive::declare_secrets!("secretspec.toml");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load secrets using the builder pattern
    let secretspec = SecretSpec::builder()
        .with_provider("keyring")  // Can use provider name or URI like "dotenv:/path/to/.env"
        .with_profile(Profile::Production)  // Can use string or Profile enum
        .load()?;

    // Access secrets (field names are lowercased)
    println!("Database: {}", secretspec.secrets.database_url);  // DATABASE_URL → database_url
    println!("Stripe: {}", secretspec.secrets.stripe_api_key);  // STRIPE_API_KEY → stripe_api_key

    // Optional secrets are Option<String>
    if let Some(redis) = &secretspec.secrets.redis_url {
        println!("Redis: {}", redis);
    }

    // Access profile and provider information
    println!("Using profile: {}", secretspec.profile);
    println!("Using provider: {}", secretspec.provider);

    // For backwards compatibility, export as environment variables
    secretspec.secrets.set_as_env_vars();

    Ok(())
}
```

Add to your `Cargo.toml`:
```toml
[dependencies]
secretspec = "0.2.0"
secretspec_derive = "0.2.0"
```

The application code never specifies *where* to get secrets - only *what* it needs through the TOML file. This keeps your application logic clean and portable.

### Building SDKs for Other Languages

We'd love to see more SDKs that bring this same declarative approach to Python, JavaScript, Go, and other languages.

## A world of possibilities

We're exploring features for future workflows:

- [Secret rotation without shutting down the application](https://github.com/cachix/secretspec/issues/11)
- [Generating secrets](https://github.com/cachix/secretspec/issues/9)
- [Mixing providers](https://github.com/cachix/secretspec/issues/10)


## Final words

Let's make secret management as declarative as package management. Let's stop sharing `.env` files over Slack. Let's build better tools for developers.

Share your thoughts on our [Discord community](https://discord.gg/naMgQehY) or [open an issue on GitHub](https://github.com/cachix/secretspec/issues). We'd love to hear how you handle secrets in your team.

Domen
