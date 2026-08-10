{ pkgs, ... }:

let
  lowerCaseLibrary = name: pkgs.runCommand "lowercase-library-${name}" { } ''
    mkdir -p "$out/library"
    touch "$out/library/${name}"
  '';
in
{
  packages = [
    (lowerCaseLibrary "first")
    (lowerCaseLibrary "second")
  ];

  enterTest = ''
    test -f "$DEVENV_PROFILE/library/first"
    test -f "$DEVENV_PROFILE/library/second"

    test -n "$DEVELOPER_DIR"
    test -n "$SDKROOT"
    test -e "$SDKROOT/usr/include/stdio.h"

    xcrun --find clang >/dev/null

    printf '#include <stdio.h>\nint main(void) { puts("ok"); }\n' \
      | cc -x c - -o "$DEVENV_STATE/apple-sdk-profile-test"
    test "$("$DEVENV_STATE/apple-sdk-profile-test")" = "ok"
  '';
}
