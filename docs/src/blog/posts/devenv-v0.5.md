---
draft: false 
date: 2022-12-22
authors:
  - domenkozar
---

# devenv 0.5

# Highlights

- ``devenv search`` now shows results from the options that can be set in `devenv.nix`:

![devenv search results](https://user-images.githubusercontent.com/126339/208920765-69044213-8977-4bb1-bd40-22cc00104ae4.png)

- [bobvanderlinden](https://github.com/bobvanderlinden/) added [Nix Flakes](https://www.tweag.io/blog/2020-05-25-flakes/) support and [wrote a guide](https://devenv.sh/guides/using-with-flakes/) how to get started.

- [thenonameguy](https://github.com/thenonameguy) rewrote how [the developer environment](https://github.com/cachix/devenv/pull/191
) is set up so that it doesn't pollute unnecessary environment variables and improves performance.

- [thenonameguy](https://github.com/thenonameguy) contributed [nix-direnv](https://github.com/nix-community/nix-direnv) integration that will speed up loading of the developer environment.

- [domenkozar](https://github.com/domenkozar) further improved [Nix error messages to include the relevant error at the bottom](https://github.com/NixOS/nix/pull/7494).

- [zimbatm](https://github.com/zimbatm) and [R-VdP](https://github.com/R-VdP) reduced the number of nixpkgs instances ([see why it's important](https://zimbatm.com/notes/1000-instances-of-nixpkgs)) to 1.

## Languages

- Rust language support now integrates with [fenix](https://github.com/nix-community/fenix) to provide stable/nightly/unstable toolchain for `cargo`, `rustc`, `rust-src`, `rust-fmt`, `rust-analyzer` and `clippy`.

- Python language now sets `$PYTHONPATH` to point to any installed packages in `packages` attribute.

- Ruby langauge support now defaults to the latest version `3.1.x`, ships with [an example running rails](https://github.com/cachix/devenv/blob/main/examples/ruby/devenv.nix), sets `$GEM_HOME` and `$GEM_PATH` environment variables. Next release will support picking [any version of Ruby](https://github.com/cachix/devenv/issues/220) - please leave a thumbs up.

- [jpetrucciani](https://github.com/jpetrucciani) contributed [Nim](https://nim-lang.org/), [V](https://vlang.io/) and [HCL/Terraform](https://github.com/hashicorp/hcl) languages support.  

## Services

- [zimbatm](https://github.com/zimbatm) moved [all existing services](https://github.com/cachix/devenv/pull/200) into `services.*` option namespace.

- [shyim](https://github.com/shyim) contributed services for [minio](https://min.io/), [MailHog](https://github.com/mailhog/MailHog), [adminer](https://www.adminer.org/), [memcached](https://memcached.org/), [blackfire](https://www.blackfire.io/), [elasticsearch](https://www.elastic.co/), [rabbitmq](https://www.rabbitmq.com/) and [cadddy](https://caddyserver.com/). Phew!

- [bobvanderlinden](https://github.com/bobvanderlinden/) contributed [wiremock](https://wiremock.org/).

##  Integrations 

- [alejandrosame](https://github.com/alejandrosame) contributed [starship](https://starship.rs/) integration

- [domenkozar](https://github.com) added [difftastic](https://github.com/Wilfred/difftastic) integration.

- [shyim](https://github.com/shyim) improved [gitpod](https://www.gitpod.io/) integration for devenv repository.

- [rkrzr](https://github.com/rkrzr
) added [hivemind](https://github.com/DarthSim/hivemind) [process.implementation](https://devenv.sh/reference/options/#installation) option.

- [domenkozar](https://github.com/domenkozar) added [an example](https://github.com/cachix/devenv/tree/main/examples/nur) how to integrate [NUR](https://nur.nix-community.org/).

# Bug fixes

- [shyim](https://github.com/shyim) fixed [MySql sleep on macOS](https://github.com/cachix/devenv/pull/226).

- [domenkozar](https://github.com/domenkozar) disabled [update checking when using flakes](https://github.com/cachix/devenv/pull/208) and fixed `devenv` to warn correctly if CLI is newer than the `devenv.lock` pin.

- [mdavezac](https://github.com/mdavezac) fixed macOS readlink bug using the wrong command.

- [domenkozar](https://github.com/domenkozar) fixed `devenv shell` to propagate exit code back to the main shell.

- [bobvanderlinden](https://github.com/bobvanderlinden/) removed version information when loading the environment, as now that's redudant due to `devenv info` command.
