---
title: "Binary Caching"
---

Most [packages](/packages/) come pre-built with binaries provided by [the official Nix binary cache](https://cache.nixos.org).

If you're modifying a package or using a package that's not built upstream,
Nix will build it from source instead of downloading a binary.

To prevent packages from being built more than once, devenv provides seamless integration with
binary caches hosted by [Cachix](https://cachix.org).

## Setup with SecretSpec (recommended)

:::tip[New in version 2.2]
:::


Sign up on [Cachix](https://cachix.org), create an organization and your first cache.

You don't need to install Cachix client, devenv will handle binary caching for you.

Create either [a personal auth token](https://app.cachix.org/personal-auth-tokens)
or a per-cache token in the cache settings.

Enable [SecretSpec](/integrations/secretspec/) with the system keyring as the
provider, and enable devenv's built-in Cachix token requirement:

```yaml title="devenv.yaml"
secretspec:
  enable: true
  provider: keyring
  cachix_auth_token: true
```

No `secretspec.toml` declaration is needed. Run `devenv shell`; on the first
interactive run, devenv prompts for the token and saves it to the keyring.
Subsequent runs resolve it automatically. Non-interactive invocations, such as
direnv, cannot prompt, so run `devenv shell` once first.

devenv uses the resolved secret for both pulling from private caches and
pushing, without exporting it into your environment.

Setting `secretspec.cachix_auth_token` to `true` uses the secret name
`CACHIX_AUTH_TOKEN`. Set it to a string instead when your SecretSpec backend's policy
(e.g. OpenBao/Vault) only grants access to the token under a different name:

```yaml title="devenv.yaml"
secretspec:
  enable: true
  provider: openbao
  cachix_auth_token: MY_TEAM_CACHIX_TOKEN
```

## Setup (legacy)

Export [a personal auth token](https://app.cachix.org/personal-auth-tokens) or
a per-cache token as `CACHIX_AUTH_TOKEN=XXX`.

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

:::note

[devenv.cachix.org](https://devenv.cachix.org) is added to the list of pull caches by default.

It mirrors the official NixOS cache and is designed to provide caching for the [`devenv-nixpkgs/rolling`](https://github.com/cachix/devenv-nixpkgs) nixpkgs input.

Some languages and integrations may automatically add caches when enabled.
:::

## Multi-user Nix and trusted users

devenv registers the caches in `cachix.pull` with Nix as substituters, together with their public signing keys.
On a multi-user install (the default on macOS, and `--daemon` installs on Linux) the Nix daemon performs
the builds, and it only accepts substituters and keys requested by users listed in
[`trusted-users`](https://nix.dev/manual/nix/latest/command-ref/conf-file#conf-trusted-users), or
substituters already present in `trusted-substituters`. For anyone else the daemon drops the request and
builds every path that is missing from `cache.nixos.org` locally, without an error in the shell.

Check whether the daemon trusts you:

```sh
$ nix store info
...
Trusted: 1
```

If it reports `Trusted: 0`, either make yourself a trusted user or configure the caches in the daemon's own
configuration.

### Adding yourself to `trusted-users`

Pass it when installing Nix:

```sh
# nix-installer
$ curl -sSfL https://artifacts.nixos.org/nix-installer | sh -s -- install --extra-conf "trusted-users = root $USER"
```

Or on an existing install, append it to the daemon's configuration and restart the daemon.
On macOS with nix-installer that file is `/etc/nix/nix.custom.conf`; on Linux it is `/etc/nix/nix.conf`:

```sh
$ echo "trusted-users = root $USER" | sudo tee -a /etc/nix/nix.conf
$ sudo systemctl restart nix-daemon      # Linux
$ sudo launchctl kickstart -k system/org.nixos.nix-daemon   # macOS
```

On NixOS or nix-darwin, set `nix.settings.trusted-users = [ "root" "yourname" ];` instead.

A trusted user can add arbitrary paths to the Nix store, so grant it only to the person who uses the machine.

### Configuring the caches in the daemon

Alternatively, add the caches and their public keys to the daemon's configuration so they apply to every
user without trusting anyone. The key is shown on the cache's page at `https://app.cachix.org/cache/<name>`:

```
extra-substituters = https://devenv.cachix.org
extra-trusted-public-keys = devenv.cachix.org-1:w1cLUi8dv3hnoSPGAuibQv+f9TZLr6cv/Hm9XgU50cw=
```

Single-user installs (`--no-daemon`) have no daemon and are not affected.

## Pushing


```nix title="devenv.nix"
{
  cachix.push = "mycache";
}
```

### Pushing binaries conditionally

You'll likely not want every user to push to the cache.

It's usually convenient to enable pushing [explicitly](/files-and-variables/#devenvlocalnix), for example as part of CI run:

```sh
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
