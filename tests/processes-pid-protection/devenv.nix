{ pkgs, ... }:

{
  # Define a simple long-running process for testing
  processes.test-process = {
    exec = ''
      while true; do
        echo "running..."
        sleep 1
      done
    '';
  };
}
