{ pkgs, lib, ... }:

{
  packages = lib.optionals pkgs.stdenv.isLinux [ pkgs.inotify-tools ];

  languages.elixir.enable = true;

  services.postgres = {
    enable = true;
    initialScript = ''
      CREATE ROLE postgres WITH LOGIN PASSWORD 'postgres' SUPERUSER;
    '';
  };

  processes.phoenix.exec = "cd hello && mix phx.server";

  enterShell = ''
    if [ ! -d "hello" ]; then
      mix local.hex --force
      mix local.rebar --force
      mix archive.install --force hex phx_new
      mix phx.new --install hello
    fi
  '';
}
