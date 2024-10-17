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
    pkgs.darwin.apple_sdk.frameworks.Security
  ];
}
```


## macOS patterns

### Link against macOS system frameworks

When compiling for macOS, you may need to link against system frameworks, like CoreFoundation.
These frameworks can be found in `pkgs.darwin.apple_sdk.frameworks`.

Add the frameworks you need to `packages` and Nix will configure the shell with necessary linker flags.

```nix
{ pkgs, lib, ... }:

{
  packages = [
    # Other dependencies
  ] ++ lib.optionals pkgs.stdenv.isDarwin [
    pkgs.darwin.apple_sdk.frameworks.CoreFoundation
    pkgs.darwin.apple_sdk.frameworks.Security
    pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
  ];
}
```

### Run x86 binaries on ARM Macs via Rosetta

It's possible to tell Nix to use Intel packages on macOS machines running on ARM.

```nix
{ pkgs, ... }:

let
  rosettaPkgs =
    if pkgs.stdenv.isDarwin && pkgs.stdenv.isAarch64
    then pkgs.pkgsx86_64Darwin
    else pkgs;
in {
  packages = [
    pkgs.git
    rosettaPkgs.vim
  ];
}
```
