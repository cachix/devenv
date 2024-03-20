Typically [packages](./packages.md) come prebuilt with binaries provided by [the official binary cache](https://cache.nixos.org).

If you're modifying a package or using a package that's not built upstream,
Nix will build it from source instead of downloading a binary.

To prevent packages from being built more than once, there's seamless integration with
binary caches using [Cachix](https://cachix.org).

## Setup

If you'd like to push binaries to your own cache, you'll need [to create one](https://app.cachix.org/cache).

After that you'll need to set `cachix authtoken XXX` with either [a personal auth token](https://app.cachix.org/personal-auth-tokens) or a cache token (that you can create in cache settings).

## devenv.nix

To specify `pre-commit-hooks` as a cache to pull from and `mycache` to pull from and push to:

```nix title="devenv.nix"
{
  cachix.pull = [ "pre-commit-hooks" ];
  cachix.push = "mycache";
}
```

# Pushing only in specific cases

You'll likely not want every user to push to the cache.

It's usually convenient to push [explicitly](./files-and-variables/#devenvlocalnix), for example as part of CI run:

```shell-session
$ echo '{ cachix.push = "mycache"; }' > devenv.local.nix
```