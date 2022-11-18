{ ... }:

{
  postgres.enable = true;

  # A shorthand for running psql connected to the devenv PostgreSql server.
  scripts.psql.exec = "${pkgs.postgresql}/bin/psql -h $(realpath .devenv/state/postgres/) $@";
}
