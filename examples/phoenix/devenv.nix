{ pkgs, lib, ... }:

# Install Phoenix dependencies:

# mix local.hex
# mix local.rebar
# mix archive.install hex phx_new
#
# Follow the instructions from https://hexdocs.pm/phoenix/up_and_running.html
# Run `mix phx.new hello --install` to create a new Phoenix project
{
  packages = [
    pkgs.git
  ] ++ lib.optionals pkgs.stdenv.isLinux [ pkgs.inotify-tools ];

  languages.elixir.enable = true;

  services.postgres = {
    enable = true;
    initialScript = ''
      CREATE ROLE postgres WITH LOGIN PASSWORD 'postgres' SUPERUSER;
    '';
    initialDatabases = [{ name = "hello_dev"; }];
  };

  processes.phoenix.exec = "cd hello && mix phx.server";
}
