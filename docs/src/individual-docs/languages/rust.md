
# Rust

The `languages.rust` module provides comprehensive support for [Rust](https://www.rust-lang.org/) development, offering flexible toolchain management through two distinct approaches.

## Getting started

Enable Rust support in your `devenv.nix`:

```nix
{
  languages.rust.enable = true;
}
```

This will provide a complete Rust development environment with `rustc`, `cargo`, `clippy`, `rustfmt`, and `rust-analyzer`.

## Toolchain management

devenv supports two approaches for managing Rust toolchains:

### 1. nixpkgs channel (default)

The `nixpkgs` channel is easy to set up and uses the Rust version currently available in your nixpkgs revision. However, it's limited to the version in nixpkgs.

```nix
{
  languages.rust = {
    enable = true;
    channel = "nixpkgs"; # default
  };
}
```

### 2. rust-overlay channels

For more control over versions and features, use the `stable`, `beta`, or `nightly` channels powered by [rust-overlay](https://github.com/oxalica/rust-overlay):

- ✅ Rustup-like channel selection
- ✅ Access to any Rust version
- ✅ Support for cross-compilation targets

```nix
{
  languages.rust = {
    enable = true;
    channel = "stable";
    version = "1.81.0"; # or "latest"
  };
}
```

Since devenv 2.4.1, a `major.minor` version such as `"1.81"` selects the newest `1.81.x` release, as it does with rustup.

### Custom toolchain packages

Set `languages.rust.toolchainPackage` to supply an aggregate toolchain built elsewhere.
Since devenv 2.4.1, custom packages and `toolchainFile` profiles use the selected compiler's sysroot for standard-library source discovery instead of injecting sources from another toolchain.
Include `rust-src` in these toolchains for standard-library navigation in rust-analyzer.

If your sources are installed separately, set `env.RUST_SRC_PATH` to the matching standard-library source directory. This setting also overrides the default source path for component-based toolchains.

## Examples

### Basic setup with latest stable

```nix
{
  languages.rust = {
    enable = true;
    channel = "stable";
  };
}
```

### Nightly Rust with extra components

```nix
{
  languages.rust = {
    enable = true;
    channel = "nightly";
    components = [ "rustc" "cargo" "clippy" "rustfmt" "rust-analyzer" "miri" ];
  };
}
```

### Cross-compilation setup

```nix
{
  languages.rust = {
    enable = true;
    channel = "stable";
    targets = [ "wasm32-unknown-unknown" "aarch64-unknown-linux-gnu" ];
  };
}
```

### Minimal installation

```nix
{
  languages.rust = {
    enable = true;
    channel = "stable";
    components = [ "rustc" "cargo" "rust-std" ];
  };
}
```

## Toolchain from project files

Rust projects describe their toolchain in two places, and devenv can read both:

- `rust-toolchain.toml` declares the toolchain you develop with. Use `languages.rust.toolchainFile`.
- `Cargo.toml` declares the minimum supported Rust version (`rust-version`). Use `languages.rust.cargo.rustVersion` to test against it.

### Using rust-toolchain.toml

If your project uses a `rust-toolchain.toml` file, devenv can automatically configure the toolchain from it:

```nix
{
  languages.rust = {
    enable = true;
    toolchainFile = ./rust-toolchain.toml;
  };
}
```

Example `rust-toolchain.toml`:
```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
profile = "minimal"
```

### Testing against your MSRV

Since devenv 2.4.1, `languages.rust.cargo.rustVersion` reads the `rust-version` field from `Cargo.toml`, so you can pin the toolchain to your minimum supported Rust version:

```nix
{ config, ... }:
{
  languages.rust = {
    enable = true;
    channel = "stable";
    version = config.languages.rust.cargo.rustVersion ./.;
  };
}
```

Pass the directory containing `Cargo.toml`, or the file itself. Workspaces are resolved the way Cargo does it: a member with `rust-version.workspace = true` takes the version from its workspace root, and the root itself reads `[workspace.package]`. Versions such as `1.70` are expanded to `1.70.0`, so the toolchain is the exact minimum release.

## Reading Cargo.toml and Cargo.lock

Since devenv 2.4.1, the functions under `languages.rust.cargo` read Cargo project files during evaluation, so other parts of your configuration can depend on them. They work without `languages.rust.enable`, and devenv tracks the files it reads: editing `Cargo.toml` or `Cargo.lock` invalidates the evaluation cache.

### Matching a tool to a locked dependency

Some tools must match the version of a library in your `Cargo.lock` exactly: `wasm-bindgen-cli` and `wasm-bindgen`, `sqlx-cli` and `sqlx`, `diesel_cli` and `diesel`. `lockedVersion` reads the locked version so you can assert the two stay in sync:

```nix
{ config, pkgs, ... }:
let
  wasmBindgenVersion = config.languages.rust.cargo.lockedVersion ./. "wasm-bindgen";
  wasmBindgenCli = pkgs.wasm-bindgen-cli_0_2_100;
in
{
  packages = [ wasmBindgenCli ];

  assertions = [
    {
      assertion = wasmBindgenCli.version == wasmBindgenVersion;
      message = "wasm-bindgen-cli ${wasmBindgenCli.version} does not match wasm-bindgen ${wasmBindgenVersion} in Cargo.lock";
    }
  ];
}
```

`Cargo.lock` lives at the workspace root, so a workspace member directory is searched upwards until the lock file is found. Evaluation fails with a list of versions if the crate is locked at more than one.

### Anything else in the files

`readManifest` and `readLock` return the parsed files as attribute sets, for everything the helpers above do not cover:

```nix
{ config, ... }:
let
  manifest = config.languages.rust.cargo.readManifest ./.;
  lock = config.languages.rust.cargo.readLock ./.;
in
{
  env.APP_VERSION = manifest.package.version;
  env.APP_EDITION = manifest.package.edition;
  env.LOCKED_CRATES = toString (builtins.length lock.package);
}
```

## Integration with other tools

### Git hooks

Rust tools integrate seamlessly with [git hooks](/reference/options/#git-hookshooks):

```nix
{
  languages.rust.enable = true;

  git-hooks.hooks = {
    rustfmt.enable = true;
    clippy.enable = true;
  };
}
```

[comment]: # (Please add your documentation on top of this line)

@AUTOGEN_OPTIONS@

### Language server with toolchain files

Since devenv 2.4.1, toolchain files that omit `rust-analyzer`, including minimal profiles, use the standalone `pkgs.rust-analyzer` language server by default.
If the file includes rust-analyzer, devenv uses the server from that toolchain instead.
To keep the server tied to a pinned compiler version, list `rust-analyzer` in the file's components.
Set `languages.rust.lsp.package` to select another server, or disable it with `languages.rust.lsp.enable = false`.
