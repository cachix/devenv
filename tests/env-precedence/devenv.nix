{ pkgs, ... }:
{
  # stdenv's setup and the setup hooks of packages set some variables after
  # `env` does. The values in `env` must win, and a value that a hook only
  # extends must keep what the hook added.
  packages = [
    pkgs.pkg-config
    pkgs.zlib
  ];

  env = {
    # Replaced by the setup hooks of the compiler and binutils wrappers.
    CC = "zig cc";
    CXX = "/custom/bin/c++";
    AR = "custom-ar";
    # Replaced by stdenv's setup before any hook of the shell runs.
    CONFIG_SHELL = "/custom/bin/sh";
    # Extended by the setup hook of pkg-config.
    PKG_CONFIG_PATH = "/custom/lib/pkgconfig";
  };

  enterTest = ''
    fail() { echo "FAIL: $*" >&2; exit 1; }

    [[ "$CC" == "zig cc" ]] || fail "CC is '$CC'"
    [[ "$CXX" == /custom/bin/c++ ]] || fail "CXX is '$CXX'"
    [[ "$AR" == custom-ar ]] || fail "AR is '$AR'"
    [[ "$CONFIG_SHELL" == /custom/bin/sh ]] || fail "CONFIG_SHELL is '$CONFIG_SHELL'"

    [[ "$PKG_CONFIG_PATH" == /custom/lib/pkgconfig:* ]] \
      || fail "PKG_CONFIG_PATH is '$PKG_CONFIG_PATH'"
    pkg-config --exists zlib || fail "pkg-config does not find zlib"
  '';
}
