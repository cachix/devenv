{ pkgs, ... }:

{
  languages.ruby.enable = true;

  # turn off C tooling if you do not intend to compile native extensions, enabled by default
  # languages.c.enable = false;

  enterShell = ''
    echo 'Making sure the basics for native compilation are available:'

    which gcc
    gcc --version

    which clang
    clang --version

    which make
    make --version

    bundle
  '';
}
