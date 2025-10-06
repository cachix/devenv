{
  processes.dummy.exec = "sleep 60";

  enterTest = ''
    # Test that running devenv up -d twice should fail when processes are already running

    # Start processes in detached mode
    devenv up -d

    # Wait a moment for processes to start
    sleep 2

    # Try to start again - this should fail with an error about processes already running
    if devenv up -d 2>&1 | grep -q "Processes already running"; then
      echo "✓ Second 'devenv up -d' correctly detected running processes"
    else
      echo "✗ Second 'devenv up -d' did not detect running processes"
      devenv processes down
      exit 1
    fi

    # Stop the processes
    devenv processes down

    # Wait for processes to fully stop
    sleep 2

    # Now we should be able to start processes again
    devenv up -d

    # Verify it started successfully
    if [ -f .devenv/processes.pid ]; then
      echo "✓ Processes started successfully after stopping"
    else
      echo "✗ Failed to start processes after stopping"
      exit 1
    fi

    # Clean up
    devenv processes down
  '';
}
