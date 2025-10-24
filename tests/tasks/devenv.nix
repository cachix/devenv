{ lib, pkgs, ... }:
{
  tasks = {
    # Test: Basic bash task execution
    "test:basic-execution" = {
      exec = "touch test-basic.txt";
    };

    # Test: Working directory option (cwd)
    "test:cwd" = {
      exec = "pwd > $DEVENV_ROOT/cwd-test.txt";
      cwd = "/tmp";
    };

    # Test: Status option prevents execution when it returns 0
    "test:status-skip" = {
      exec = "touch should-not-exist.txt";
      status = "exit 0";
    };

    # Test: Task dependencies with "before" and "after" using task outputs
    "test:dep-first" = {
      exec = ''
        echo '{"order": 1}' > "$DEVENV_TASK_OUTPUT_FILE"
      '';
    };

    "test:dep-second" = {
      exec = ''
        echo '{"order": 2}' > "$DEVENV_TASK_OUTPUT_FILE"
      '';
      after = [ "test:dep-first" ];
    };

    "test:dep-third" = {
      exec = ''
        echo '{"order": 3}' > "$DEVENV_TASK_OUTPUT_FILE"
      '';
      before = [ "test:dep-verify" ];
      after = [ "test:dep-second" ];
    };

    "test:dep-verify" = {
      exec = ''
        # Verify all three previous tasks ran in order by checking their outputs
        first=$(echo "$DEVENV_TASKS_OUTPUTS" | ${lib.getExe pkgs.jq} -r '."test:dep-first".order // "missing"')
        second=$(echo "$DEVENV_TASKS_OUTPUTS" | ${lib.getExe pkgs.jq} -r '."test:dep-second".order // "missing"')
        third=$(echo "$DEVENV_TASKS_OUTPUTS" | ${lib.getExe pkgs.jq} -r '."test:dep-third".order // "missing"')

        if [ "$first" != "1" ] || [ "$second" != "2" ] || [ "$third" != "3" ]; then
          echo "Dependency order incorrect!"
          echo "Expected: first=1, second=2, third=3"
          echo "Got: first=$first, second=$second, third=$third"
          exit 1
        fi
      '';
    };

    # Test: Python runner
    "test:python-success" = {
      exec = ''
        import sys
        with open('python-output.txt', 'w') as f:
            f.write('Hello from Python!\n')
            f.write(f'Python version: {sys.version}\n')
        print('Task completed successfully')
      '';
      package = pkgs.python3;
    };

    # Test: Python error handling
    "test:python-error" = {
      exec = ''
        import sys
        print('This task will fail intentionally', file=sys.stderr)
        sys.exit(1)
      '';
      package = pkgs.python3;
    };

    "test:with-output" = {
      exec = ''
        echo "VISIBLE_OUTPUT_MARKER"
      '';
      showOutput = true;
    };

    "test:without-output" = {
      exec = ''
        echo "HIDDEN_OUTPUT_MARKER"
      '';
      showOutput = false;
    };
  };
}
