## Getting a recent version of a package from `nixpkgs-unstable`

By default, devenv [uses a fork of nixpkgs](https://devenv.sh/blog/2024/03/20/devenv-10-rewrite-in-rust/#devenv-nixpkgs) with additional fixes. This fork can be several months behind `nixpkgs-unstable`. You can still get a more recently updated package from `nixpkgs-unstable` into your devenv.

1. Add `nixpkgs-unstable` input to `devenv.yaml`:

   ```yaml
   inputs:
     nixpkgs:
       url: github:cachix/devenv-nixpkgs/rolling
     nixpkgs-unstable:
       url: github:nixos/nixpkgs/nixpkgs-unstable
   ```

2. Use the package in your `devenv.nix`:

   ```nix
   { pkgs, inputs, ... }:
   let
     pkgs-unstable = import inputs.nixpkgs-unstable { system = pkgs.stdenv.system; };
   in
   {
     packages = [
       pkgs-unstable.elmPackages.elm-test-rs
     ];
   }
   ```

## Nix patterns

### Add a directory to `$PATH`

This example adds Elixir install scripts to `~/.mix/escripts`:

```nix
{ ... }:

{
  languages.elixir.enable = true;

  enterShell = ''
    export PATH="$HOME/.mix/escripts:$PATH"
  '';
}
```

### Escape Nix curly braces inside shell scripts

```nix
{ pkgs, ... }: {
  scripts.myscript.exec = ''
    foobar=1
    echo ''${foobar}
  '';
}
```


## Container patterns

### Exclude packages from a container

```nix
{ pkgs, ... }: {
  packages = [
    pkgs.git
  ] ++ lib.optionals (!config.container.isBuilding) [
    pkgs.haskell-language-server
  ];
}
```


## Cross-platform patterns

### Configure the shell based on the current machine

Some packages are available only on certain processor architectures or operating systems.
A number of helper functions exist in `pkgs.stdenv` to help you dynamically configure the shell based on the current machine.

A few of the most commonly used functions are:

- `stdenv.isLinux` to target machines running Linux
- `stdenv.isDarwin` to target machines running macOS

- `stdenv.isAarch64` to target ARM64 processors
- `stdenv.isx86_64` to target X86_64 processors

```nix
{ pkgs, lib, ... }: {
  packages = [
    pkgs.ncdu
  ] ++ lib.optionals pkgs.stdenv.isLinux [
    pkgs.inotify-tools
  ] ++ lib.optionals pkgs.stdenv.isDarwin [
    pkgs.libiconv
  ];
}
```


## macOS patterns

### Link against macOS system frameworks

When compiling for macOS, you may need to link against system frameworks, like CoreFoundation and Security.
These frameworks are shipped in a versioned SDK bundle available as `pkgs.apple-sdk`.

You can use the [`apple.sdk`](reference/options.md#applesdk) option to override the default SDK or remove it completely.

```nix
{ pkgs, lib, ... }:

{
  # Use a different SDK version.
  apple.sdk =
    if pkgs.stdenv.isDarwin
    then pkgs.apple-sdk_15
    else null;

  # Remove the default Apple SDK.
  # This allows you to use the system SDK at the cost of reducing reproducibility.
  # apple.sdk = null;
}
```

!!! note "Legacy framework pattern"

    You previously had to add each framework to `packages` individually. For example:

    ```nix
    { pkgs, lib, ... }:

    {
      packages = lib.optionals pkgs.stdenv.isDarwin [
        pkgs.darwin.apple_sdk.frameworks.CoreFoundation
      ];
    }
    ```

    This is no longer necessary. Frameworks are bundled together in a single versioned SDK.


### Run x86 binaries on Apple Silicon with Rosetta

Rosetta 2 enables a Mac with Apple Silicon to transparently run x86 binaries.

Nixpkgs provides a convenient set of x86_64-darwin packages.
This can come in handy for packages that don't yet have an aarch64-compatible build or are temporarily broken on nixpkgs.

```nix
{ pkgs, lib, ... }:

let
  rosettaPkgs = pkgs.pkgsx86_64Darwin;
in {
  packages = [
    pkgs.git
  ] ++ lib.optionals (pkgs.stdenv.isDarwin && pkgs.stdenv.isAarch64) [
    rosettaPkgs.dmd
  ];
}
```
