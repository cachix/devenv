# `devenv` Integration for [Conan](https://conan.io/) via [conan-flake](https://codeberg.org/tarcisio/conan-flake)

## Set up

Add `conan-flake` to your inputs:

```shell-session
$ devenv inputs add conan-flake git+https://codeberg.org/tarcisio/conan-flake
```

Check [the list of available options](/reference/options.md#conanenable).

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

## Additional Devenv Config Examples

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
