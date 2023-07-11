{ pkgs, lib, ... }:

{
  languages.ruby.enable = true;
  languages.ruby.version = "3.2.2";

  services.postgres.enable = true;

  processes.rails.exec = "rails server";

  enterShell = ''
    export PATH="$DEVENV_ROOT/blog/bin:$PATH"
    bundle
  '';
}
