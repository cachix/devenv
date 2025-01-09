Most [packages](./packages.md) come pre-built with binaries provided by [the official Nix binary cache](https://cache.nixos.org).

If you're modifying a package or using a package that's not built upstream,
Nix will build it from source instead of downloading a binary.

To prevent packages from being built more than once, devenv provides seamless integration with
binary caches hosted by [Cachix](https://cachix.org).

# Setup

Devenv will automatically configure Cachix caches for you, or guide you how to add the caches to Nix manually.
Any caches set up by devenv are used in addition to the caches configured in Nix, for example, in `/etc/nix/nix.conf`.

## Pull

To pull binaries from [pre-commit-hooks.cachix.org](https://pre-commit-hooks.cachix.org), add it to `cachix.pull`:

```nix title="devenv.nix"
{
  cachix.enable = true;
  cachix.pull = [ "pre-commit-hooks" ];
}
```

### The `devenv` cache

[devenv.cachix.org](https://devenv.cachix.org) is added to the list of pull caches by default.
It mirrors the official NixOS cache and is designed to provide caching for the [`devenv-nixpkgs/rolling`](https://github.com/cachix/devenv-nixpkgs) nixpkgs input.

Some languages and integrations may automatically add caches when enabled.

## Pushing

If you'd like to push binaries to your own cache, you'll need [to create one](https://app.cachix.org/cache).

After that you'll need to set `cachix authtoken XXX` with either [a personal auth token](https://app.cachix.org/personal-auth-tokens) or a cache token (that you can create in cache settings).

```nix title="devenv.nix"
cachix.enable = true
cachix.push = "mycache";
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
cachix.enable = false;
```

Nix will continue to substitute binaries from any caches you may have configured externally, such as the official NixOS cache.
