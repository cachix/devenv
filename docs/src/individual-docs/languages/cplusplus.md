## Getting Started

The easiest way to get started with C++ is to simply enable it:

```nix
languages.cplusplus = {
  enable = true;
};
```

This will automatically:

- Use `clang` as the default C++ package
- Install it along with CMake and other tools

Alternatively, you can manually specify packages:

```nix
languages.cplusplus = {
  enable = true;
  package = pkgs.stdenv.cc;
};
```

## Setting up the [Conan](https://conan.io/) package manager

Add `conan-flake` to your inputs:

```shell-session
$ devenv inputs add conan-flake git+https://codeberg.org/tarcisio/conan-flake
```

You can check [the list of available options](/reference/options.md#languagescplusplusconanenable). The [`languages.cplusplus.conan.config`](/reference/options.md#languagescplusplusconanconfig) option, however, maps the whole of the options available in the [`conan-flake`](https://flake.parts/options/conan-flake.html) module &mdash; check the [official module documentation](https://flake.parts/options/conan-flake.html#options) and see the examples in [conan-flake's README file](https://codeberg.org/tarcisio/conan-flake/src/branch/main/README.md) to help you setting up.

Config the `devenv.nix` file accordingly. For example:

```nix
languages.cplusplus = {
  enable = true;
  conan = {
    enable = true;
    install.enable = true;
  };
};
```

By default, when Conan is enabled:

- The default C++ package is set to `config.stdenv.cc`
- Conan is configured to use the same CMake available in the developmemnt shell

### In Action:

```shell-session
$ devenv shell
Building shell ...
Entering shell ...

conan profile show # This would show the default profile.
```

## Additional Examples

### LLVM-based C++ Toolchain

If you would like to integrate with the LLVM compiler infrastructure:

```nix
{ pkgs, ... }:

{
  languages.cplusplus = {
    enable = true;
    conan = {
      enable = true;
      install.enable = true;
      config = {
        stdenv = pkgs.overrideCC
          (
            pkgs.llvmPackages.libcxxStdenv.override {
              targetPlatform.useLLVM = true;
            }
          )
          pkgs.llvmPackages.clangUseLLVM;
        # By default: compiler.libcxx=libstdc++11, so undo it:
        compilerLibCxx = null;
      };
    };
  };
}
```

Or even:

```nix
{ pkgs, ... }:

{
  stdenv = pkgs.overrideCC
    (
      pkgs.llvmPackages.libcxxStdenv.override {
        targetPlatform.useLLVM = true;
      }
    )
    pkgs.llvmPackages.clangUseLLVM;

  languages.cplusplus = {
    enable = true;
    conan = {
      enable = true;
      install.enable = true;
      config = {
        # By default: compiler.libcxx=libstdc++11, so undo it:
        compilerLibCxx = null;
      };
    };
  };
}
```

### In Action:

```shell-session
$ devenv shell
Building shell ...
Entering shell ...

conan profile show # This would show the default profile.
conan create . --build=missing # This would create and test the current package.
```

[comment]: # (Please add your documentation on top of this line)

@AUTOGEN_OPTIONS@
