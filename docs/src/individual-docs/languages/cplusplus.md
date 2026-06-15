## Getting Started

To get started with C++ in devenv, you can enable it in the `languages.cplusplus` namespace:

```nix title="devenv.nix"
languages.cplusplus = {
  enable = true;
};
```

This will automatically:

- Use `clang` as the default C++ package.
- Install it along with CMake ([`languages.cplusplus.cmake`](/reference/options.md#languagescpluspluscmake)), the C++ Language Server ([`languages.cplusplus.lsp`](/reference/options.md#languagescpluspluslspenable)) and the standalone command line tools for C++ development ([`languages.cplusplus.tools`](/reference/options.md#languagescplusplustoolsenable)).

Alternatively, you can manually specify packages:

```nix title="devenv.nix"
languages.cplusplus = {
  enable = true;
  package = pkgs.stdenv.cc;
};
```

## Setting up the [Conan](https://conan.io/) package manager

Add conan-flake to your inputs:

```shell-session
$ devenv inputs add conan-flake git+https://codeberg.org/tarcisio/conan-flake
```

The conan-flake module bridges the gap between Nix and Conan, supporting a declarative configuration style. For instance, for a user profile configuration like the following:

```ini
[settings]
build_type=Debug
compiler.cppstd=14

[platform_tool_requires]
cmake/X.Y.Z
```

There corresponds the following conan-flake options:

```nix
{
  profiles = {
    settings.compiler."compiler.cppstd" = "14";
    settings.rest.build_type = "Debug";

    platformToolRequires = {
      cmake = pkgs.cmake.version;
    };
  };

  devShell = {
    # Programs you want to make available in the shell:
    tools = { inherit (pkgs) cmake; };
  };
}
```

You can check [the list of available options](/reference/options.md#languagescplusplusconanenable). Observe that the [`languages.cplusplus.conan.config`](/reference/options.md#languagescplusplusconanconfig) option maps the whole of the options available in the [conan-flake](https://flake.parts/options/conan-flake.html) module &mdash; check the [official module documentation](https://flake.parts/options/conan-flake.html#options) and see the examples in [conan-flake's README file](https://codeberg.org/tarcisio/conan-flake/src/branch/main/README.md) to help you setting up.

Set your `devenv.nix` file accordingly. For example, the above is actually equivalent to the following:

```nix title="devenv.nix"
{ pkgs, config, ... }:
{
  languages.cplusplus = {
    enable = true;
    conan = {
      enable = true;
      install.enable = true;
      config = {
        profiles = {
          settings.compiler."compiler.cppstd" = "14";
          settings.rest.build_type = "Debug";
        };
      };
    };
  };
}
```

By default, when `languages.cplusplus.conan` is enabled:

- The C++ package is set to `config.stdenv.cc` &mdash; that is, the system C compiler configured for devenv to use for the developer environment.
- Whenever devenv is configured without a C compiler toolchain (see the recipe [Skip the C compiler toolchain](../recipes/nix.md#skip-the-c-compiler-toolchain) for an example), the C++ package is defaulted to `pkgs.stdenv.cc` instead.
- Conan is configured to use the `languages.cplusplus.cmake` package available in the developer shell; as can be seen from the above example, the devenv integration automatically takes care of the CMake part, and the `profiles.platformToolRequires` and `devShell.tools` options are not required to be set explicitly.

### In Action:

```shell-session
$ devenv shell
$ conan profile show
Host profile:
[settings]
arch=x86_64
build_type=Debug
compiler=gcc
compiler.cppstd=14
compiler.libcxx=libstdc++11
compiler.version=15.2.0
os=Linux
[platform_tool_requires]
cmake/4.1.2

Build profile:
[settings]
arch=x86_64
build_type=Debug
compiler=gcc
compiler.cppstd=14
compiler.libcxx=libstdc++11
compiler.version=15.2.0
os=Linux
[platform_tool_requires]
cmake/4.1.2
```

## Additional Examples

### LLVM-based C++ Toolchain

If you would like to integrate with the LLVM compiler infrastructure:

```nix title="devenv.nix"
{ pkgs, config, lib, ... }:
{
  languages.cplusplus = {
    enable = true;
    conan = {
      enable = true;
      install.enable = true;
      config = {
        profiles = {
          settings.rest.build_type = "Release";
        };
        stdenv = pkgs.overrideCC
          (
            pkgs.llvmPackages.libcxxStdenv.override {
              targetPlatform.useLLVM = true;
              targetPlatform.linker = "lld";
            }
          )
          pkgs.llvmPackages.clangUseLLVM;
      };
    };
  };
}
```

Or even:

```nix title="devenv.nix"
{ pkgs, config, lib, ... }:
{
  stdenv = pkgs.overrideCC
    (
      pkgs.llvmPackages.libcxxStdenv.override {
        targetPlatform.useLLVM = true;
        targetPlatform.linker = "lld";
      }
    )
    pkgs.llvmPackages.clangUseLLVM;

  languages.cplusplus = {
    enable = true;
    conan = {
      enable = true;
      install.enable = true;
      config = {
        profiles = {
          settings.rest.build_type = "Release";
        };
      };
    };
  };
}
```

In this second use case:

- By default, conan-flake is configured using the same stdenv as devenv's (that is, `config.stdenv`).

### In Action:

```shell-session
$ devenv shell
$ conan create . --build=missing
...
======== Testing the package: Building ========

======== Testing the package: Executing test ========
example/0.0.1 (test package): Running test()
example/0.0.1 (test package): RUN: example
hello-world: Hello World Release!
  hello-world: __x86_64__ defined
  hello-world: __cplusplus202002
  hello-world: __GNUC__4
  hello-world: __GNUC_MINOR__2
  hello-world: __clang_major__21
  hello-world: __clang_minor__1
example/0.0.1 test_package
```

- The `conan create` command creates and tests the current package.

### A local-recipe-index remote

With [local-recipe-index](https://docs.conan.io/2/tutorial/conan_repositories/setup_local_recipes_index.html) remotes it's possible to declare dependencies from a simplified local index structure:

```nix title="devenv.nix"
{
  languages.cplusplus = {
    enable = true;

    conan = {
      enable = true;
      install.enable = true;

      config = {
        profiles = {
          settings.compiler."compiler.cppstd" = "17";
          settings.rest.build_type = "Release";
        };

        remotes.local = {
          url = "./repo";
          local = true;
          allowedPackages = [
            "hello-world/0.0.1.cci.20260428"
          ];
        };

        offline = true;
      };
    };
  };
}
```

The options:

- `remotes.local.url`: is taken as a relative path to the root of the configuration.
- `offline`: enable only local remotes (that is, only of local-recipe-index type).

### In Action:

```shell-session
$ devenv shell
$ conan remote list
conancenter: https://center2.conan.io [Verify SSL: True, Enabled: False]
local: /path/to/config/root/./repo [local-recipes-index, Enabled: True, Allowed packages: hello-world/0.0.1.cci.20260428]
```

[comment]: # (Please add your documentation on top of this line)

@AUTOGEN_OPTIONS@
