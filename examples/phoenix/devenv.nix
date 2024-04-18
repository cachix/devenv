{ pkgs, lib, ... }:

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

  enterShell = ''
    mix local.hex --force
    mix local.rebar --force
    mix archive.install --force hex phx_new

    if [ ! -d "hello" ]; then
      # guard against multiple invocation of "enterShell"
      mkdir hello

      echo y | mix phx.new --install hello
      sed -i.bak -e "s/hostname: \"localhost\"/socket_dir: System.get_env(\"PGHOST\")/" \
        ./hello/config/dev.exs && rm ./hello/config/dev.exs.bak
    fi
  '';
}
