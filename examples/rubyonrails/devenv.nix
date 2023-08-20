{ pkgs, lib, ... }:

{
  languages.ruby.enable = true;
  languages.ruby.version = "3.2.2";

  packages = [
    pkgs.openssl
  ];

  services.postgres.enable = true;

  processes.rails.exec = "rails server";

  enterShell = ''
    if [ ! -d "blog" ]; then
      rails new blog -d=postgresql
    fi
    export PATH="$DEVENV_ROOT/blog/bin:$PATH"
    pushd blog
      bundle
    popd
  '';
}
