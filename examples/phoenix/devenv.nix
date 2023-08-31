{ pkgs, lib, ... }:

{
  packages = lib.optionals pkgs.stdenv.isLinux [ pkgs.inotify-tools ];

  languages.elixir.enable = true;

  services.postgres.enable = true;

  processes.phoenix.exec = "cd hello && mix ecto.create && mix phx.server";
}
