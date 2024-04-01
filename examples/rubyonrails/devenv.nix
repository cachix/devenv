{ pkgs, lib, ... }:

{
  languages.ruby.enable = true;
  languages.ruby.version = "3.2.2";

  packages = [
    pkgs.openssl
    pkgs.libyaml
    pkgs.git
    pkgs.curl
  ];

  services.postgres.enable = true;

  processes.rails.exec = "rails server";

  enterShell = ''
    if [ ! -d "blog" ]; then
      gem install rails || exit 1
      rails new blog --database=postgresql --force || exit 1
    fi
    export PATH="$DEVENV_ROOT/blog/bin:$PATH"
    pushd blog
      bundle || exit 1
    popd
  '';
}
