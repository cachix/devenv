Most [packages](./packages.md) come pre-built with binaries provided by [the official Nix binary cache](https://cache.nixos.org).

If you're modifying a package or using a package that's not built upstream,
Nix will build it from source instead of downloading a binary.

To prevent packages from being built more than once, devenv provides seamless integration with
binary caches hosted by [Cachix](https://cachix.org).

# Setup

Sign up on [Cachix](https://cachix.org), create an organization and your first cache.

You don't need to install Cachix client, devenv will handle binary caching for you.

After that you'll need to set `CACHIX_AUTH_TOKEN=XXX` with either [a personal auth token](https://app.cachix.org/personal-auth-tokens) or a per cache token (that you can create in cache settings).

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
