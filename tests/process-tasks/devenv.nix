{ pkgs, ... }:

{
  # Define a process that writes to a file
  processes.test-process = {
    exec = ''
      echo "process executed" >> output.txt
      # Keep the process running so we can test it
      sleep 1
    '';
  };

  # Define a task that should run before the process
  tasks."myapp:test-before-task" = {
    exec = ''
      echo "task executed" >> output.txt
    '';
    # This task should run before the process
    before = [ "devenv:processes:test-process" ];
  };

  # Test script to verify the order
  enterTest = ''
    # Clean up any existing output file
    rm -f output.txt

    # Wait a bit for processes to start and tasks to run
    sleep 2

    # Check the output file for correct order
    if [ -f output.txt ]; then
      content=$(cat output.txt)
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
