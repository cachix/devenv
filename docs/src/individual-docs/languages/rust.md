
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

## Integration with other tools

### Git hooks

Rust tools integrate seamlessly with [git hooks](/reference/options.md/#git-hookshooks):

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
