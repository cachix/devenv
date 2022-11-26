{ pkgs, ... }:

{
  process.implementation = "process-compose";

  processes.foo.exec = "echo foo; sleep 5";

  postgres.enable = true;

  processes.bar = {
    exec = (pkgs.writeShellScript "complex-process" ''
      # testing multiline bash scripts
      echo "I'm $PC_PROC_NAME, replica: $PC_REPLICA_NUM"

      echo "how many files did postgres create?"
      ls "$PGDATA" | wc -l

      echo 'showing off process-specific env var:'
      echo "$BAR"
    '').outPath;

    process-compose = {
      depends_on.foo.condition = "process_completed_successfully";
      depends_on.postgres.condition = "process_ready";
      environment = [ "BAR=BAZ" ];
    };
  };
}
