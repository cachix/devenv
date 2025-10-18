## Cross-platform patterns

### Configure the shell based on the current machine

Some packages are available only on certain processor architectures or operating systems.
A number of helper functions exist in `pkgs.stdenv` to help you dynamically configure the shell based on the current machine.

A few of the most commonly used functions are:

+ `stdenv.isLinux` to target machines running Linux
+ `stdenv.isDarwin` to target machines running macOS

+ `stdenv.isAarch64` to target ARM64 processors
+ `stdenv.isx86_64` to target X86_64 processors

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
  ``` { .console .no-copy }
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

```nix title="devenv.nix" hl_lines="3 9"
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

