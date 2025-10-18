## macOS patterns

### Link against macOS system frameworks

When compiling for macOS, you may need to link against system frameworks, like CoreFoundation and Security.
These frameworks are shipped in a versioned SDK bundle available as `pkgs.apple-sdk`.

You can use the [`apple.sdk`](/reference/options.md#applesdk) option to override the default SDK or remove it completely.

```nix title="devenv.nix"
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

<div class="result" markdown>

!!! note "Legacy framework pattern"

    You previously had to add each framework to `packages` individually. For example:

    ```nix title="devenv.nix"
    { pkgs, lib, ... }:

    {
      packages = lib.optionals pkgs.stdenv.isDarwin [
        pkgs.darwin.apple_sdk.frameworks.CoreFoundation
      ];
    }
    ```

    This is no longer necessary. Frameworks are bundled together in a single versioned SDK.

</div>

### Run x86 binaries on Apple Silicon with Rosetta

Rosetta 2 enables a Mac with Apple Silicon to transparently run x86 binaries.

Nixpkgs provides a convenient set of x86_64-darwin packages.
This can come in handy for packages that don't yet have an aarch64-compatible build or are temporarily broken on nixpkgs.

```nix title="devenv.nix"
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

### Compile x86 dependencies on ARM Macs via Rosetta

If you want to also compile dependencies for x86, you can add dependencies to `packages`:

```nix
packages = with rosettaPkgs; [
    freetds
    krb5
    openssl
];
```

And switch the compiler to x86:

```nix
stdenv = rosettaPkgs.stdenv;
```

Then, in the generated shell, you can compile software for x86, like `pymssql` or `ibm_db`.
