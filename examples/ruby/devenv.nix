{ pkgs, ... }:

{
  languages.ruby = {
    enable = true;
    compilers.enable = true;
  };

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
