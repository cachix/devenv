---
draft: false
date: 2025-08-22
authors:
  - domenkozar
---

# Closing the Nix Gap: From Environments to Packaged Applications for Rust

<blockquote class="twitter-tweet"><p lang="en" dir="ltr">Should I use crate2nix, cargo2nix, or naersk for packaging my Rust application?</p>&mdash; (@jvmncs) <a href="https://twitter.com/jvmncs/status/1927120951918891508">January 21, 2025</a></blockquote>
<script async src="https://platform.twitter.com/widgets.js" charset="utf-8"></script>

This tweet shows a common problem in Nix: "Should I use crate2nix, cargo2nix, or naersk for packaging my Rust application?"

devenv solved this for development environments differently: instead of making developers package everything with Nix, we provide tools through a simple `languages.rust.enable`. You get `cargo`, `rustc`, and `rust-analyzer` in your shell without understanding Nix packaging.

But when you're ready to deploy, you face the same problem: which lang2nix tool should you use? Developers don't want to compare `crate2nix` vs `cargo2nix` vs `naersk` vs `crane`—they want a tested solution that works.

devenv now provides `languages.rust.import`, which packages Rust applications using [crate2nix](https://github.com/nix-community/crate2nix). We evaluated the available tools and chose crate2nix, so you don't have to.

We've done this before. In [PR #1500](https://github.com/cachix/devenv/pull/1500), we replaced [fenix](https://github.com/nix-community/fenix) with [rust-overlay](https://github.com/oxalica/rust-overlay) for Rust toolchains because rust-overlay was better maintained. Users didn't need to change anything—devenv handled the transition while keeping the same `languages.rust.enable = true` interface.

## One Interface for All Languages

The typical workflow:

1. **Development**: Enable the language (`languages.rust.enable = true`) to get tools like `cargo`, `rustc`, and `rust-analyzer`.
2. **Packaging**: When ready to deploy, use `languages.rust.import` to package with Nix.

The same pattern works for all languages:

```nix
{ config, ... }: {
  # https://devenv.sh/languages
  languages = {
    rust.enable = true;
    python.enable = true;
    go.enable = true;
  };

  # https://devenv.sh/outputs
  outputs = {
    rust-app = config.languages.rust.import ./rust-app {};
    python-app = config.languages.python.import ./python-app {};
    go-app = config.languages.go.import ./go-app {};
  };
}
```

## Starting with Rust

`languages.rust.import` automatically generates Nix expressions from `Cargo.toml` and `Cargo.lock`.

Add the crate2nix input:

```shell-session
$ devenv inputs add crate2nix github:nix-community/crate2nix --follows nixpkgs
```

Import your Rust application:

```nix
{ config, ... }:
let
  # ./app is the directory containing your Rust project's Cargo.toml
  myapp = config.languages.rust.import ./app {};
in
{
  # Provide developer environment
  languages.rust.enable = true;

  # Expose our application inside the environment
  packages = [ myapp ];

  # https://devenv.sh/outputs
  outputs = {
    inherit myapp;
  };
}
```

Build your application:

```shell-session
$ devenv build outputs.myapp
```

## Other Languages

This API extends to other languages, each using the best packaging tool:

We've also started using [uv2nix](https://github.com/pyproject-nix/uv2nix) to provide a similar interface for Python in [PR #2115](https://github.com/cachix/devenv/pull/2115).

## That's it

For feedback, join our [Discord community](https://discord.gg/naMgvexb6q).

Domen
