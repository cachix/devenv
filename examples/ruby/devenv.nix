{ pkgs, ... }:

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
}
