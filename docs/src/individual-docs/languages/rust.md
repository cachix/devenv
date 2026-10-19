
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

Since devenv 2.4.1, `languages.rust.cargoRustVersion` reads the `rust-version` field from the `Cargo.toml` in a directory, so you can pin the toolchain to your minimum supported Rust version:

```nix
{ config, ... }:
{
  languages.rust = {
    enable = true;
    channel = "stable";
    version = config.languages.rust.cargoRustVersion ./.;
  };
}
```

Point it at a directory whose `Cargo.toml` sets `rust-version`. For a workspace, that is the workspace root with `rust-version` under `[workspace.package]`. Members that inherit it with `rust-version.workspace = true` are not resolved, so pass the root directory instead (for example `../..` from `crates/foo/devenv.nix`). Versions such as `1.70` are expanded to `1.70.0`.

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
