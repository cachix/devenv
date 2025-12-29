{ config, ... }:

let
  outputPath = "${config.devenv.state}/output.txt";
in
{
  # Define a process that writes to a file
  processes.test-process = {
    exec = ''
      echo "process executed" >> ${outputPath}
      sleep 10
    '';
  };

  # Define a task that should run before the process
  tasks."myapp:test-before-task" = {
    exec = ''
      echo "task executed" >> ${outputPath}
    '';
    # This task should run before the process
    before = [ "devenv:processes:test-process" ];
  };

  # Test script to verify the order
  enterTest = ''
    wait_for_processes

    # Check the output file for correct order
    if [ -f ${outputPath} ]; then
      content=$(cat ${outputPath})
      expected=$'task executed\nprocess executed'

      if [ "$content" = "$expected" ]; then
        echo "✓ Tasks ran in correct order"
      else
        echo "✗ Tasks did not run in correct order"
        echo "Expected:"
        echo "$expected"
        echo "Got:"
        echo "$content"
        exit 1
      fi
    else
      echo "✗ output.txt was not created"
      exit 1
    fi
  '';
}
