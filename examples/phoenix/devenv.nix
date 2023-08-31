{ pkgs, lib, ... }:

{
  packages = lib.optionals pkgs.stdenv.isLinux [ pkgs.inotify-tools ];

  languages.elixir.enable = true;

  services.postgres = {
    enable = true;
    listen_addresses = "127.0.0.1";
  };

  processes.phoenix.exec = "cd hello && mix ecto.create && mix phx.server";
}
