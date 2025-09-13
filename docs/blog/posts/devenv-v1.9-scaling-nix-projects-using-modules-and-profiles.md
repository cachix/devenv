---
date: 2025-09-14
authors:
  - domenkozar
draft: false
---

# devenv 1.9: Scaling Nix projects using modules and profiles

[Profiles](/profiles/) are a new way to organize and selectively activate parts of development environment.

While we try [our best to ship sane defaults for languages and services](https://en.wikipedia.org/wiki/Convention_over_configuration), each team has its own preferences. We're [still working on uniform interface for language configuration](https://github.com/cachix/devenv/pull/1974) so you'll be able to customize each bit of the environment.

Typically, these best practices are created using scaffolds, these quickly go out of date and don't have
the ability to ship updates in a central place.

On top of that, when developing in a repository with different components, it's handy to be able to activate only part of
the development environment.

## Extending devenv modules

Teams can define their own set of recommended best practices in a central repository to create even more opinionated environments:

```nix title="devenv.nix"
{ lib, config, pkgs, ... }: {
  options.myteam = {
    languages.rust.enable = lib.mkEnableOption "Rust development stack";
    services.database.enable = lib.mkEnableOption "Database services";
  };

  config = {
    packages = lib.mkIf config.myteam.languages.rust.enable [
      pkgs.cargo-watch
    ];

    languages.rust = lib.mkIf config.myteam.languages.rust.enable {
      enable = true;
      channel = "nightly";
    };

    services.postgres = lib.mkIf config.myteam.services.database.enable {
      enable = true;
      initialScript = "CREATE DATABASE myapp;";
    };
  };
}
```

We have defined our defaults for `myteam.languages.rust` and `myteam.services.database`.

## Using Profiles

Once you have your team module defined, you can start using it in new projects:

```yaml title="devenv.yaml"
inputs:
  myteam:
    url: github:myorg/devenv-myteam
    flake: false
inputs:
- myteam
```

This automatically includes your centrally managed module.

Since options default to `false`, you'll need to enable them per project. You can enable common defaults globally and use profiles to activate additional components on demand:

```nix title="devenv.nix"
{ pkgs, config, ... }: {
  packages = [ pkgs.jq ];

  profiles = {
    backend.config = {
      myteam.languages.rust.enable = true;
      myteam.services.database.enable = true;
    };

    frontend.config = {
      languages.javascript.enable = true;
    };

    fullstack.extends = [ "backend" "frontend" ];
  };
}
```

Let's do some Rust development with the base configuration:

```shell-session
$ devenv --profile backend shell
```

Using backend profile to launch the database:

```shell-session
$ devenv --profile backend up
```

Using frontend profile for JavaScript development:

```shell-session
$ devenv --profile frontend shell
```

Using fullstack profile to get both backend and frontend tools (extends both profiles):

```shell-session
$ devenv --profile fullstack shell
```

The fullstack profile automatically includes everything from both the backend and frontend profiles through extends. Use [ad-hoc environment options](../../ad-hoc-developer-environments.md) to further customize:

```shell-session
$ devenv -P fullstack -O myteam.languages.rust.enable:bool false shell
```

## User and Hostname Profiles

Profiles can activate automatically based on hostname or username:

```nix
{
  profiles = {
    hostname."dev-server".config = {
      myteam.services.database.enable = true;
    };

    user."alice".config = {
      myteam.languages.rust.enable = true;
    };
  };
}
```

When user `alice` runs `devenv shell` on `dev-server` hostname, both her user profile and the hostname profile automatically activate.

This gives teams fine-grained control over development environments while keeping individual setups simple and centralized.

## Building Linux containers on macOS

Oh, we've also [removed restriction so you can now build containers on macOS](https://github.com/cachix/devenv/pull/2085) if you configure a linux builder.

Containers are likely to get a simplification redesign, as we've learned a lot since they were [introduced in devenv 0.6](https://devenv.sh/blog/2023/03/02/devenv-06-generating-containers-and-instant-shell-activation/).

## Getting Started

New to devenv? Start with the [getting started guide](/getting-started/) to learn the basics.

Check out the [profiles documentation](/profiles) for complete examples.

Join the [devenv Discord community](https://discord.gg/naMgvexb6q) to share how your team uses profiles!

Domen
