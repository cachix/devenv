# Binary Caching

Most [packages](./packages.md) come pre-built with binaries provided by [the official Nix binary cache](https://cache.nixos.org).

If you're modifying a package or using a package that's not built upstream,
Nix will build it from source instead of downloading a binary.

To prevent packages from being built more than once, devenv provides seamless integration with
binary caches hosted by [Cachix](https://cachix.org).

## Setup with SecretSpec (recommended)

Sign up on [Cachix](https://cachix.org), create an organization and your first cache.

You don't need to install Cachix client, devenv will handle binary caching for you.

Declare a `CACHIX_AUTH_TOKEN` secret using [SecretSpec](integrations/secretspec.md).
devenv automatically resolves the secret and uses it for both pulling from
private caches and pushing, without exporting it into your environment.

The SecretSpec secret name defaults to `CACHIX_AUTH_TOKEN` and is overridable
via [`secretspec.cachix_auth_token`](reference/yaml-options.md#secretspeccachix_auth_token)
in `devenv.yaml`. Override it when your SecretSpec backend's policy
(e.g. OpenBao/Vault) only grants access to the token under a different name:

```yaml title="devenv.yaml"
secretspec:
  enable: true
  provider: openbao
  cachix_auth_token: MY_TEAM_CACHIX_TOKEN
```

## Setup (legacy)

Set `CACHIX_AUTH_TOKEN=XXX` with either [a personal auth token](https://app.cachix.org/personal-auth-tokens)
or a per-cache token that you can create in the cache settings.

If `CACHIX_AUTH_TOKEN` is not set in the environment and SecretSpec does not
provide it, devenv falls back to the auth token stored by the Cachix CLI
(`cachix authtoken`) in `$XDG_CONFIG_HOME/cachix/cachix.dhall` (usually
`~/.config/cachix/cachix.dhall`).

## Pull

Configure your new cache:

```nix title="devenv.nix"
{
  cachix.pull = [ "mycache" ];
}
```

!!! note 

    [devenv.cachix.org](https://devenv.cachix.org) is added to the list of pull caches by default. 

    It mirrors the official NixOS cache and is designed to provide caching for the [`devenv-nixpkgs/rolling`](https://github.com/cachix/devenv-nixpkgs) nixpkgs input.

    Some languages and integrations may automatically add caches when enabled.

## Pushing


```nix title="devenv.nix"
{
  cachix.push = "mycache";
}
```

### Pushing binaries conditionally

You'll likely not want every user to push to the cache.

It's usually convenient to enable pushing [explicitly](files-and-variables.md#devenvlocalnix), for example as part of CI run:

```shell-session
$ echo '{ cachix.push = "mycache"; }' > devenv.local.nix
```

## Disabling the Cachix integration

You can disable the integration by setting the following in `devenv.nix`:

```nix title="devenv.nix"
{
  cachix.enable = false;
}
```

Nix will continue to substitute binaries from any caches you may have configured externally, such as the official NixOS cache.
