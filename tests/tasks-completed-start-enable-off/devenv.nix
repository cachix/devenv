{
  # One-shot process kept out of `devenv up` (`start.enable = false`) and never
  # auto-restarted. Creates process-ran.txt when it actually runs.
  # Regression for https://github.com/cachix/devenv/issues/3005
  processes.marker = {
    exec = "touch process-ran.txt";
    start.enable = false;
    restart.on = "never";
  };

  # Should run the process to completion first (`@completed`), then its own body.
  tasks."repro:build" = {
    after = [ "devenv:processes:marker@completed" ];
    exec = "touch task-ran.txt";
  };
}
