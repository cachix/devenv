---
draft: false 
date: 2023-03-02
authors:
  - domenkozar
---

# devenv 0.6: Generating containers and instant shell activation

After about two months of active development, I'm happy to announce [devenv 0.6](/getting-started/) is ready.

This release comes with the most notable improvements based on the feedback from existing users:

- Adding the ability to [generate containers](#generating-containers).
- [Instant shell activation](#instant-shell-activation) of the developer environment.
- [Hosts and ceritifcates](#hosts-and-certificates) provisioning.
- [New devenv.yaml options](#allowunfree-and-overlays): `allowUnfree` and `overlays`.

## Generating containers

While `devenv shell` provides a [simple native developer environment](/basics/) experience,
`devenv container <name>` allows you to generate and copy [OCI container](https://opencontainers.org/) into a registry.

Containers are a great way to distribute ready-made applications, leveraging platforms like [fly.io](https://github.com/cachix/devenv/tree/main/examples/fly.io) to deploy them into production.

An example for Ruby:

```nix title="devenv.nix"
{
  name = "simple-ruby-app";

  languages.ruby.enable = true;
  languages.ruby.version = "3.2.1";
}
```

We can generate a container called `shell` that enters the environment, copy it to the local Docker daemon and run it:


```
$ devenv container shell --docker-run
...
(devenv) bash-5.2# ruby --version
ruby 3.2.1 (2023-02-08 revision 31819e82c8) [x86_64-linux]
```

You can read more in the new [Containers](/containers/) section of the documentation, specifically:

- [How to generate a container for shell](/containers/#entering-the-development-environment)
- [How to generate a container to start all processes](/containers/#running-processes)
- [How to generate a container to start a single process](/containers/#running-a-single-process)
- [How to generate a container to start a custom built binary](/containers/#running-artifacts)
- [How to copy the containers to a registry](/containers/#copying-container-to-a-registry)
- [How to conditionalize environment based on native/container target](/containers/#changing-environment-based-on-the-build-type)

## Instant shell activation

Especially **monorepo** developer environments can sometimes be even **a few gigabytes** of size, taking **a few seconds** for the environment to be activated.

A developer **environment should only be built
when something changes** and if not, the environment
can be used **instantly using a cached snapshot**.

With the latest [direnv.net integration](/automatic-shell-activation/),
we've **finally reached that goal** by making caching work properly (it will even watch each of your imports for changes!). 

!!! note "Migrating from an older devenv"
    Make sure to use the latest `.envrc` from `devenv init` and for everyone on the team to [upgrade to devenv 0.6](/getting-started/).

In the near future we'll experiment to improve [devenv shell](https://github.com/cachix/devenv/issues/240) experience.

## Hosts and certificates

Hosts and certificates can now be specified declaratively:

```nix
{ pkgs, config, ... }:

{
  certificates = [
    "example.com"
  ];

  hosts."example.com" = "127.0.0.1";

  services.caddy.enable = true;
  services.caddy.virtualHosts."example.com" = {
    extraConfig = ''
      tls ${config.env.DEVENV_STATE}/mkcert/example.com.pem ${config.env.DEVENV_STATE}/mkcert/example.com-key.pem
      respond "Hello, world!"
    '';
  };
}
```

And when you run `devenv up` to start [the processes](/processes/), these hosts and certificates will be provisioned locally.

## `allowUnfree` and `overlays`

For example in `devenv.yaml`:

```yaml
allowUnfree: true
inputs:
  nixpkgs:
    url: github:NixOS/nixpkgs/nixpkgs-unstable
  rust-overlay:
    url: github:oxalica/rust-overlay
    overlays:
      - default
```

Will allow building unfree software and wire up `default` overlay into `pkgs` from [rust-overlay](https://github.com/oxalica/rust-overlay).


!!! note "Migrating from an older devenv"
    Make sure *everyone* on the team upgrades [to devenv 0.6](/getting-started/).


## Languages changelog

- **Python:** Added support for virtualenv creation and poetry by [bobvanderlinden](https://github.com/bobvanderlinden/).
- **Ruby:** First-class support for setting `version` or `versionFile` by [bobvanderlinden](https://github.com/bobvanderlinden/).
- **Go:** Received significant improvements by [shyim](https://github.com/shyim).
- **PHP:** Added first-class support for setting version to make it easier to set extensions by [shyim](https://github.com/shyim).
- **Scala:** Now allows changing the package and offers scala-cli as an option if the JDK is too old by [domenkozar](https://github.com/domenkozar).
- **R:** Added an option to specify the package by [adfaure](https://github.com/adfaure).
- **Rust:** Can now find headers for darwin frameworks by [domenkozar](https://github.com/domenkozar).
- **OCaml:** Allowed using a different version of OCaml by [ankhers](https://github.com/ankhers).
- **Tex Live:** Added support by [BurNiinTRee](https://github.com/BurNiinTRee).
- **Swift:** Added support by [domenkozar](https://github.com/domenkozar).
- **Raku:** Added support by [0pointerexception](https://github.com/0pointerexception).
- **Gawk:** Added support by [0pointerexception](https://github.com/0pointerexception).
- **Racket:** Added support by [totoroot](https://github.com/totoroot).
- **Dart:** Added support by [domenkozar](https://github.com/domenkozar).
- **Julia:** Added support by [domenkozar](https://github.com/domenkozar).
- **Crystal:** Added support by [bcardiff](https://github.com/bcardiff).
- **Unison:** Added support by [ereslibre](https://github.com/ereslibre).
- **Zig:** Added support by [ereslibre](https://github.com/ereslibre).
- **Deno:** Added support by [janathandion](https://github.com/janathandion).

## Services changelog

- **Cassandra:** Added by [ankhers](https://github.com/ankhers).

- **CouchDB**: Added by [MSBarbieri](https://github.com/MSBarbieri).

- **MariaDB:** Corrected user and database handling by [jochenmanz](https://github.com/jochenmanz).

- **MinIO:** Now allows specifying what buckets to provision by [shyim](https://github.com/shyim).

## Fixed issues and other improvements

- process-compose: Faster shutdown, restart on failure by default, escape env variables properly by [thenonameguy](https://github.com/thenonameguy).

- Support assertions in modules by [bobvanderlinden](https://github.com/bobvanderlinden).

- Fix overmind root by [domenkozar](https://github.com/domenkozar).

- Make `devenv info` output pluggable from devenv modules by [domenkozar](https://github.com/domenkozar).

- Expand the flake guide by [sandydoo](https://github.com/sandydoo).

- Set `LOCALE_ARCHIVE` when missing by [sandydoo](https://github.com/sandydoo).

- Numerous option documentation fixes by [sandydoo](https://github.com/sandydoo).

- Fix starship integration with a custom config by [domenkozar](https://github.com/domenkozar).

- Test direnv integration with strict bash mode by [stephank](https://github.com/stephank).

- Add a shim `devenv` for flakes integration by [rgonzalez](https://github.com/rgonzalez).