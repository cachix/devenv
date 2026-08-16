{ pkgs, ... }:
{
  packages = [
    # `meta.outputsToInstall = [ "out" ]`; the `unbundled`, `p11kit` and `hashed`
    # outputs collide with each other and must stay out of the profile.
    pkgs.cacert
    # bin, dev, out, man, doc, debug: `out` and `dev` are linked because the
    # shell exposes them, `doc` and `debug` are not.
    pkgs.openssl
    # An explicitly selected output is linked as-is.
    pkgs.sqlite.dev
  ];

  enterTest = ''
    # cacert: only the default output
    test -e "$DEVENV_PROFILE/etc/ssl/certs/ca-bundle.crt"
    test ! -e "$DEVENV_PROFILE/etc/ssl/trust-source"
    if ls "$DEVENV_PROFILE"/etc/ssl/certs/*.0 >/dev/null 2>&1; then
      echo "hashed cacert output was linked into the profile" >&2
      exit 1
    fi

    # openssl: binaries, libraries, headers and man pages, but no docs or debug info
    test -x "$DEVENV_PROFILE/bin/openssl"
    ls "$DEVENV_PROFILE"/lib/libssl.* >/dev/null
    test -e "$DEVENV_PROFILE/include/openssl/ssl.h"
    test -e "$DEVENV_PROFILE/share/man/man1/openssl.1ssl.gz"
    test ! -e "$DEVENV_PROFILE/share/doc/openssl"
    test ! -e "$DEVENV_PROFILE/lib/debug"

    # sqlite.dev: headers, but not the `sqlite3` binary from the `bin` output
    test -e "$DEVENV_PROFILE/include/sqlite3.h"
    test ! -e "$DEVENV_PROFILE/bin/sqlite3"

    # no output of any package collides with another
    if log=$(nix log "$DEVENV_PROFILE" 2>/dev/null) && grep -q "colliding subpath" <<<"$log"; then
      echo "profile build reported colliding subpaths" >&2
      exit 1
    fi
  '';
}
