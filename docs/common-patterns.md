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

```nix title="devenv.nix" hl_lines="4 6 14"
{ pkgs, lib, ... }: {
  packages = [
    pkgs.ncdu
  ] ++ lib.optionals pkgs.stdenv.isLinux [
    pkgs.inotify-tools
  ] ++ lib.optionals pkgs.stdenv.isDarwin [
    pkgs.libiconv
  ];

  services.postgres = {
    enable = true;
    settings = {
      log_connections = true;
    } // lib.optionalAttrs pkgs.stdenv.isLinux {
      # Additional settings for Linux systems
    };
  };
}
```

### Advanced conditional configuration with `mkIf` and `mkMerge`

For more complex cross-platform configurations, it may be tempting to use `//` and `optionalAttrs` in the top-level configuration.
This approach will cause Nix to fail with the dreaded `infinite recursion` error:

```nix title="devenv.nix" hl_lines="6"
# ❌ This will fail with "error: infinite recursion encountered"
{ pkgs, lib, ... }:

{
  packages = [ pkgs.git ];
} // lib.optionalAttrs pkgs.stdenv.isLinux {
  packages = [ pkgs.ncdu ];
  env.SOME_VAR = "linux-only";
}
```

<div class="result" >
  ```
  error: infinite recursion encountered
  ```
</div>

The reason this doesn't work is that Nix needs to evaluate the config to figure out the value of conditions like `pkgs.stdenv.isLinux`.
Despite Nix being a lazy language, it needs to be able to strictly evaluate the spine of the top-level attrset—essentially, its keys.
This can't happen when the structure itself depends on one of its values.

The solution is to use the module-specific helpers `lib.mkIf` and `lib.mkMerge`.
`mkIf` pushes the conditional into the values of the attrset, allowing evaluation to proceed.
This function adds extra metadata to the attrset, which is why you then merge multiple conditional blocks with `mkMerge`.

Use this pattern when you need to conditionally define entire configuration sections, rather than just adding packages or values within existing sections.

```nix title="devenv.nix" hl_lines="4 9"
{ pkgs, lib, ... }:

lib.mkMerge [
  {
    # Common packages
    packages = [ pkgs.git ];
  }
  (lib.mkIf pkgs.stdenv.isLinux {
    # Additional Linux packages
    packages = [ pkgs.ncdu ];
    env.SOME_VAR = "linux-only";
  })
]
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
