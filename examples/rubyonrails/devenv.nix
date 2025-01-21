{ pkgs, lib, ... }:

# Create a new Rails project:
#
# gem install rails
# rails new blog --database=postgresql --force
# cd blog
# bundle
{
  languages.ruby.enable = true;
  languages.ruby.version = "3.2.2";

  packages = [
    pkgs.openssl
    pkgs.libyaml
    pkgs.git
    pkgs.curl
    pkgs.redis
  ];

  services.postgres.enable = true;

  processes.rails = {
    exec = "cd blog && rails server";
    process-compose.depends_on.postgres.condition = "process_healthy";
  };

  enterShell = ''
    export PATH="$DEVENV_ROOT/blog/bin:$PATH"
  '';
}
