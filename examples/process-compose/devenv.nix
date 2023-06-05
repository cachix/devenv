{ pkgs, ... }:

{
  process.implementation = "process-compose";

  processes.foo.exec = "echo foo; sleep 5";

  services.postgres.enable = true;
  services.memcached.enable = true;

  languages.ruby.enable = true;

  packages = [ pkgs.imagemagick_light ];

  scripts.compile-rmagick.exec = "gem install --install-dir /tmp rmagick";

  processes.bar = {
    exec = (pkgs.writeShellScript "complex-process" ''
      # testing multiline bash scripts, env vars provided by process-compose
      echo "I'm $PC_PROC_NAME, replica: $PC_REPLICA_NUM"

      echo
      echo "how many files did postgres create?"
      ls "$PGDATA" | wc -l

      echo
      echo 'showing off process-specific env var:'
      echo "$BAR"

      echo
      echo 'can use scripts here as well:'
      compile-rmagick
    '').outPath;

    process-compose = {
      depends_on.foo.condition = "process_completed_successfully";
      depends_on.postgres.condition = "process_ready";
      environment = [ "BAR=BAZ" ];
    };
  };
}
