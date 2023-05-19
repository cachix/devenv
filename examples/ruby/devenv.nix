{ pkgs, lib, ... }:

{
  languages.ruby.enable = true;

  # Use a specific Ruby version.
  # languages.ruby.version = "3.2.1";

  # Use a specific Ruby version from a .ruby-version file, compatible with rbenv.
  languages.ruby.versionFile = ./.ruby-version;

  # turn off C tooling if you do not intend to compile native extensions, enabled by default
  # languages.c.enable = false;

  enterShell = ''
    # Automatically run bundler upon enterting the shell.
    bundle
  '';

  # Add required dependencies for macOS. These packages are usually provided as
  # part of the Xcode command line developer tools, in which case they can be
  # removed.
  # For more information, see the `--install` flag in `man xcode-select`.
  packages = lib.optionals pkgs.stdenv.isDarwin [
    pkgs.libllvm
  ];
}
