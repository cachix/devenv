{ pkgs, ... }:
{
  # `hello` ships a man page; `man-db` provides the `man` binary used to
  # resolve it via MANPATH.
  packages = [
    pkgs.hello
    pkgs.man-db
  ];

  enterTest = ''
    # The profile's man dir must be on MANPATH.
    echo "$MANPATH" | grep -qF "$DEVENV_PROFILE/share/man"

    # The man page of a package in the environment must be symlinked into the
    # profile and resolvable by `man` through MANPATH.
    test -e "$DEVENV_PROFILE/share/man/man1/hello.1.gz"
    man -w hello >/dev/null
  '';
}
