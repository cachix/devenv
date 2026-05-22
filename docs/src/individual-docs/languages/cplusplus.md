## Getting Started

The easiest way to get started with C++ is to simply enable it:

```nix
languages.cplusplus = {
  enable = true;
};
```

This will automatically:
- Use `clang` as the default C++ package
- Install it along with `cmake` and other tools

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

You can check [the list of available options](/reference/options.md#languagescplusplusenable). The [`languages.cplusplus.conan.config`](/reference/options.md#languagescplusplusconanconfig) option, however, maps the whole of the options available in the [`conan-flake`](https://flake.parts/options/conan-flake.html) module &mdash; check the [official module documentation](https://flake.parts/options/conan-flake.html#options) and see the examples in [conan-flake's README file](https://codeberg.org/tarcisio/conan-flake/src/branch/main/README.md) to help you setting up.

Config the `devenv.nix` file accordingly. For example, the following code would configure Conan to use the same CMake available in the developmemnt shell:

```nix
{ inputs, ... }:

{
  conan = {
    enable = true;
    config = {
      platformToolRequires = {
        cmake = pkgs.cmake.version;
      };
      devShell = {
        packages = [
          pkgs.cmake
        ];
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
```

## Additional Examples

### LLVM-based C++ Toolchain

If you would like to integrate with the LLVM compiler infrastructure:

```nix
{ inputs, pkgs, ... }:

{
  conan = {
    enable = true;
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
      platformToolRequires = {
        cmake = pkgs.cmake.version;
      };
      devShell = {
        packages = [
          pkgs.cmake
        ];
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
