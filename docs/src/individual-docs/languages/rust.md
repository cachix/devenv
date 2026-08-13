
# Rust

The `languages.rust` module adds the [Rust](https://www.rust-lang.org/) compiler and common Rust development tools to your environment. You can use the toolchain from nixpkgs or select a toolchain with rust-overlay.

## Getting started

Add this option to your `devenv.nix`:

```nix
{
  languages.rust.enable = true;
}
```

This option installs `rustc`, `cargo`, `clippy`, `rustfmt`, and `rust-analyzer`.

## Toolchain management

You can get the Rust toolchain from nixpkgs or rust-overlay.

### 1. nixpkgs channel (default)

The `nixpkgs` channel is the default. It does not require an additional input. It uses the Rust version in your nixpkgs revision.

```nix
{
  languages.rust = {
    enable = true;
    channel = "nixpkgs"; # default
  };
}
```

### 2. rust-overlay channels

Use [rust-overlay](https://github.com/oxalica/rust-overlay) when you need a specific Rust channel, version, or compilation target. Before you select the `stable`, `beta`, or `nightly` channel, add the `rust-overlay` input to your `devenv.yaml`:

```yaml title="devenv.yaml"
inputs:
  rust-overlay:
    url: github:oxalica/rust-overlay
    inputs:
      nixpkgs:
        follows: nixpkgs
```

Then select the channel in your `devenv.nix`:

```nix
{
  languages.rust = {
    enable = true;
    channel = "stable";
    version = "1.81.0";
  };
}
```

The `version` option uses `"latest"` by default. You can also set it to a specific Rust version or a nightly release date.

## Examples

### Use the latest stable toolchain

The `stable` channel uses the latest version by default:

```nix
{
  languages.rust = {
    enable = true;
    channel = "stable";
  };
}
```

### Add components to a nightly toolchain

Set the channel to `nightly`. List each component that you need in the `components` option:

```nix
{
  languages.rust = {
    enable = true;
    channel = "nightly";
    components = [ "rustc" "cargo" "clippy" "rustfmt" "rust-analyzer" "miri" ];
  };
}
```

### Add cross-compilation targets

Use the `targets` option to install additional compilation targets:

```nix
{
  languages.rust = {
    enable = true;
    channel = "stable";
    targets = [ "wasm32-unknown-unknown" "aarch64-unknown-linux-gnu" ];
  };
}
```

### Install fewer components

Use the `components` option to install only the tools that you need:

```nix
{
  languages.rust = {
    enable = true;
    channel = "stable";
    components = [ "rustc" "cargo" "rust-std" ];
  };
}
```

### Use rust-toolchain.toml

If your project has a `rust-toolchain.toml` file, set `toolchainFile` to its path. This option uses the `rust-overlay` input from the earlier example.

```nix
{
  languages.rust = {
    enable = true;
    toolchainFile = ./rust-toolchain.toml;
  };
}
```

For example, this `rust-toolchain.toml` selects the stable channel and two components:

```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
profile = "minimal"
```

## Integration with other tools

### Git hooks

[Git hooks](/reference/options.md/#git-hookshooks) can run Rust checks before each commit:

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
