---
title: "Rust"
---

<!-- Do not edit this generated file. Edit docs/src/individual-docs instead. -->



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

## Options

### languages.rust.enable



Whether to enable tools for Rust development.



*Type:*
boolean



*Default:*

```nix
false
```



*Example:*

```nix
true
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



### languages.rust.cargo.lockedVersion

Read the version a crate is locked at in ` Cargo.lock `.

Takes a directory (searched upwards for ` Cargo.lock `, see ` readLock `)
or the path to the file itself, and the crate name. Evaluation fails
if the crate is not in the lock file or is locked at several versions.

Useful to keep a tool in sync with the library version a project
depends on, for example ` wasm-bindgen-cli ` with ` wasm-bindgen `:

```nix
{ config, pkgs, ... }:
let
  wasmBindgenVersion = config.languages.rust.cargo.lockedVersion ./. "wasm-bindgen";
in
{
  packages = [ pkgs.wasm-bindgen-cli ];

  assertions = [{
    assertion = pkgs.wasm-bindgen-cli.version == wasmBindgenVersion;
    message = "wasm-bindgen-cli ${pkgs.wasm-bindgen-cli.version} does not match wasm-bindgen ${wasmBindgenVersion} in Cargo.lock";
  }];
}
```



*Type:*
function that evaluates to a(n) function that evaluates to a(n) string *(read only)*



*Default:*
a function reading Cargo project files

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



### languages.rust.cargo.readLock



Read a ` Cargo.lock ` into an attribute set, as [` builtins.fromTOML `](https://nix.dev/manual/nix/stable/language/builtins#builtins-fromTOML) returns it.
Locked packages are listed under ` package `.

Takes a directory, or the path to the file itself. Since ` Cargo.lock `
lives at the workspace root, a directory is searched upwards until a
` Cargo.lock ` is found, so a workspace member directory works.



*Type:*
function that evaluates to a(n) attribute set of anything *(read only)*



*Default:*
a function reading Cargo project files

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



### languages.rust.cargo.readManifest



Read a ` Cargo.toml ` into an attribute set, as [` builtins.fromTOML `](https://nix.dev/manual/nix/stable/language/builtins#builtins-fromTOML) returns it.

Takes the directory containing ` Cargo.toml `, or the path to the file itself.

Example usage:

```nix
{ config, ... }:
{
  env.APP_VERSION = (config.languages.rust.cargo.readManifest ./.).package.version;
}
```



*Type:*
function that evaluates to a(n) attribute set of anything *(read only)*



*Default:*
a function reading Cargo project files

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



### languages.rust.cargo.rustVersion



Read the minimum supported Rust version (` rust-version `) of a Cargo
package or workspace, in a form ` languages.rust.version ` accepts.

Takes the directory containing ` Cargo.toml `, or the path to the file
itself. Uses ` package.rust-version `, resolving
` rust-version.workspace = true ` through the workspace root the way
Cargo does, and falls back to ` workspace.package.rust-version ` for a
workspace root. Versions are padded to three components (` 1.70 `
becomes ` 1.70.0 `), so the result pins the exact minimum release.

Example usage to test against your MSRV:

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



*Type:*
function that evaluates to a(n) string *(read only)*



*Default:*
a function reading Cargo project files

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



### languages.rust.channel



The rustup toolchain to install.



*Type:*
one of “nixpkgs”, “stable”, “beta”, “nightly”



*Default:*

```nix
"nixpkgs"
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



### languages.rust.clangLinker.enable



Use Clang as the Rust linker driver on Linux.

This avoids GCC’s ` collect2 ` wrapper, which can hit ` Argument list too long `
in large Nix development environments before the final linker receives
Rust’s response file.

On x86_64 Linux (glibc), Clang uses LLD unless another linker is enabled.



*Type:*
boolean



*Default:*

```nix
pkgs.stdenv.hostPlatform.isLinux
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



### languages.rust.components



List of [Rustup components](https://rust-lang.github.io/rustup/concepts/components.html)
to install. Defaults to those available in ` nixpkgs `.



*Type:*
list of string



*Default:*

```nix
[ "rustc" "cargo" "clippy" "rustfmt" "rust-analyzer" ]
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



### languages.rust.cranelift.enable



Use [Cranelift](https://cranelift.dev/) as the codegen backend for dev builds.

Cranelift compiles significantly faster than LLVM at the cost of less optimized output.
Requires the nightly channel.



*Type:*
boolean



*Default:*

```nix
false
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



### languages.rust.cranelift.excludePackages



List of crate names that should use the LLVM backend instead of Cranelift.

Generates per-package overrides in ` .cargo/config.toml `.



*Type:*
list of string



*Default:*

```nix
[ ]
```



*Example:*

```nix
[
  "aws-lc-sys"
  "aws-lc-rs"
  "rustls"
]
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



### languages.rust.cranelift.forceBuildScriptsLlvm



Force build scripts and proc macros to use the LLVM backend.

Some build scripts may not work with Cranelift. Enable this to fall back to
LLVM for build scripts while keeping Cranelift for regular code.



*Type:*
boolean



*Default:*

```nix
false
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



### languages.rust.import



Import a Cargo project using crate2nix.

This function takes a path to a directory containing a Cargo.toml file
and returns a derivation that builds the Rust project using crate2nix.

Example usage:

```nix
let
mypackage = config.languages.rust.import ./path/to/cargo/project {};
in {
languages.rust.enable = true;
packages = [ mypackage ];
}
```



*Type:*
function that evaluates to a(n) function that evaluates to a(n) package

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



### languages.rust.lld.enable



Use [lld](https://lld.llvm.org/) as the linker.

lld is LLVM’s linker and is the recommended fast linker for Darwin.
Works on both Linux and macOS.



*Type:*
boolean



*Default:*

```nix
false
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



### languages.rust.lsp.enable



Whether to enable Rust Language Server.



*Type:*
boolean



*Default:*

```nix
true
```



*Example:*

```nix
true
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



### languages.rust.lsp.package



The Rust language server package to use.



*Type:*
package



*Default:*
Depends on the configured toolchain:

 - ` nixpkgs ` channel: ` pkgs.rust-analyzer `.
 - non-nixpkgs channel: the ` rust-analyzer ` component from the rust-overlay toolchain, with a fallback to ` pkgs.rust-analyzer ` if not present in the manifest.
 - ` toolchainFile `: the aggregated toolchain package if it contains rust-analyzer, otherwise ` pkgs.rust-analyzer `.

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



### languages.rust.mold.enable



Use [mold](https://github.com/rui314/mold) as the linker.

mold is a faster drop-in replacement for existing Unix linkers.
It is several times quicker than the LLVM lld linker.



*Type:*
boolean



*Default:*

```nix
false
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



### languages.rust.rustdocflags



Extra flags to pass to Rustdoc.



*Type:*
string



*Default:*

```nix
""
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



### languages.rust.rustflags



Extra flags to pass to the Rust compiler.



*Type:*
string



*Default:*

```nix
""
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



### languages.rust.targets



List of extra [targets](https://doc.rust-lang.org/nightly/rustc/platform-support.html)
to install. Defaults to only the native target.



*Type:*
list of string



*Default:*

```nix
[ ]
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



### languages.rust.toolchain



Rust component packages. May optionally define additional components, for example ` miri `.



*Type:*
open submodule of attribute set of package



*Default:*

```nix
nixpkgs
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



### languages.rust.toolchain.cargo



cargo package



*Type:*
null or package



*Default:*

```nix
pkgs.cargo
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



### languages.rust.toolchain.clippy



clippy package



*Type:*
null or package



*Default:*

```nix
pkgs.clippy
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



### languages.rust.toolchain.rust-analyzer



rust-analyzer package



*Type:*
null or package



*Default:*

```nix
pkgs.rust-analyzer
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



### languages.rust.toolchain.rustc



rustc package



*Type:*
null or package



*Default:*

```nix
pkgs.rustc
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



### languages.rust.toolchain.rustfmt



rustfmt package



*Type:*
null or package



*Default:*

```nix
pkgs.rustfmt
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



### languages.rust.toolchainFile



Path to a ` rust-toolchain ` or ` rust-toolchain.toml ` file for automatic toolchain configuration.

When set, devenv will use rust-overlay’s ` fromRustupToolchainFile ` to automatically
configure the toolchain based on the file contents (channel, components, targets, profile).

This follows the standard Rust toolchain file format documented at:
https://rust-lang.github.io/rustup/overrides.html#the-toolchain-file

Cannot be used together with manual ` channel ` or ` version ` configuration.

Example:

```nix
languages.rust.toolchainFile = ./rust-toolchain.toml;
```



*Type:*
null or absolute path



*Default:*

```nix
null
```



*Example:*

```nix
./rust-toolchain.toml
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



### languages.rust.toolchainPackage



The aggregated toolchain package, which includes the configured components and targets.
This is automatically set based on the channel and components configuration.



*Type:*
package

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



### languages.rust.version



Which version of rust to use, this value could be ` latest `, ` 1.81.0 `, ` 1.81 `, ` 2021-01-01 `.
A ` major.minor ` version such as ` 1.81 ` selects the newest ` 1.81.x ` release.
Only works when languages.rust.channel is NOT nixpkgs.



*Type:*
string



*Default:*

```nix
"latest"
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



### languages.rust.wild.enable



Use [wild](https://github.com/wild-linker/wild) as the linker.

wild is a very fast linker for Linux.



*Type:*
boolean



*Default:*

```nix
false
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)

### Language server with toolchain files

Since devenv 2.4.1, toolchain files that omit `rust-analyzer`, including minimal profiles, use the standalone `pkgs.rust-analyzer` language server by default.
If the file includes rust-analyzer, devenv uses the server from that toolchain instead.
To keep the server tied to a pinned compiler version, list `rust-analyzer` in the file's components.
Set `languages.rust.lsp.package` to select another server, or disable it with `languages.rust.lsp.enable = false`.
