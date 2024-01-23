{ pkgs, lib, ... }:

{
  packages = lib.optionals pkgs.stdenv.isLinux [ pkgs.inotify-tools ];

  languages.elixir.enable = true;

  services.postgres.enable = true;

  processes.phoenix.exec = "mix ecto.create && mix phx.server";
}
