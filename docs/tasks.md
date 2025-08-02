# Tasks

!!! info "New in version 1.2"

Tasks allow you to form dependencies between code, executed in parallel.

## Defining tasks

```nix title="devenv.nix"
{ pkgs, ... }:

{
  tasks."myapp:hello" = {
    exec = ''echo "Hello, world!"'';
  };
}
```

```shell-session
$ devenv tasks run myapp:hello
Running tasks     myapp:hello
Succeeded         myapp:hello         9ms
1 Succeeded                           10.14ms
```

!!! info "New in version 1.7"

You can also run all tasks in a namespace by providing just the namespace prefix:

```shell-session
$ devenv tasks run myapp
Running tasks     myapp:hello myapp:build myapp:test
Succeeded         myapp:hello           9ms
Succeeded         myapp:build         120ms
Succeeded         myapp:test          350ms
3 Succeeded                           479.14ms
```

## enterShell / enterTest

If you'd like the tasks to run as part of the `enterShell` or `enterTest`:

```nix title="devenv.nix"
{ pkgs, lib, config, ... }:

{
  tasks = {
    "bash:hello" = {
      exec = "echo 'Hello world from bash!'";
      before = [ "devenv:enterShell" "devenv:enterTest" ];
    };
  };
}
```

```shell-session
$ devenv shell
...
Running tasks     devenv:enterShell
Succeeded         devenv:pre-commit:install 25ms
Succeeded         bash:hello                 9ms
Succeeded         devenv:enterShell         13ms
3 Succeeded                                 28.14ms
```

## Using your favourite language

Tasks can also use another package for execution, for example when entering the shell:

```nix title="devenv.nix"
{ pkgs, lib, config, ... }:

{
  tasks = {
    "python:hello" = {
      exec = ''
        print("Hello world from Python!")
      '';
      package = config.languages.python.package;
    };
  };
}
```

## Avoiding running expensive `exec` via `status` check

If you define a `status` command, it will be executed first and if it returns `0`, `exec` will be skipped.

```nix title="devenv.nix"
{ pkgs, lib, config, ... }:

{
  tasks = {
    "myapp:migrations" = {
      exec = "db-migrate";
      status = "db-needs-migrations";
    };
  };
}
```

Tasks using the `status` attribute will also cache their outputs. When a task is skipped because its status command returns success, the output from the most recent successful run will be restored and passed to dependent tasks.

## Executing tasks only when files have been modified

You can specify a list of files to monitor with `execIfModified`. The task will only run if any of these files have been modified since the last successful run.

```nix title="devenv.nix"
{ pkgs, lib, config, ... }:

{
  tasks = {
    "myapp:build" = {
      exec = "npm run build";
      execIfModified = [
        "src"
        "package.json"
      ];
      # Optionally run the build in a specific directory
      cwd = "./frontend";
    };
  };
}
```

This is particularly useful for tasks that depend on specific files and don't need to run if those files haven't changed.

The system tracks both file modification times and content hashes to detect actual changes. If a file's timestamp changes but its content remains the same (which can happen when touching a file or when saving without making changes), the task will be skipped.

When a task is skipped due to no file changes, any previous outputs from that task are preserved and passed to dependent tasks, making the caching more efficient.

## Inputs / Outputs

Tasks support passing inputs and produce outputs, both as JSON objects:

- `$DEVENV_TASK_INPUT`: JSON object of  `tasks."myapp:mytask".input`.
- `$DEVENV_TASKS_OUTPUTS`: JSON object with dependent tasks as keys and their outputs as values.
- `$DEVENV_TASK_OUTPUT_FILE`: a writable file with tasks' outputs in JSON.

```nix title="devenv.nix"
{ pkgs, lib, config, ... }:

{
  tasks = {
    "myapp:mytask" = {
      exec = ''
        echo $DEVENV_TASK_INPUTS> $DEVENV_ROOT/input.json
        echo '{ "output": 1 }' > $DEVENV_TASK_OUTPUT_FILE
        echo $DEVENV_TASKS_OUTPUTS > $DEVENV_ROOT/outputs.json
      '';
      input = {
        value = 1;
      };
    };
  };
}
```

## Processes as tasks

!!! info "New in version 1.4"

All processes defined in `processes` are automatically available as tasks with the `devenv:processes:` prefix. This allows you to:

- Run individual processes as tasks
- Define dependencies between tasks and processes
- Use task features like `before`/`after` with processes

```nix title="devenv.nix"
{ pkgs, ... }:

{
  # Define a process
  processes.web-server = {
    exec = "python -m http.server 8080";
  };

  # Define a task that runs before the process
  tasks."setup-data" = {
    exec = "echo 'Setting up data...'";
    before = [ "devenv:processes:web-server" ];
  };
}
```

When you run `devenv tasks run devenv:processes:web-server`, it will:
1. First run any tasks that have `before = [ "devenv:processes:web-server" ]`
2. Then execute the process itself

This is particularly useful for:
- Running setup tasks before starting a process
- Creating complex startup sequences
- Testing individual processes without starting all of them

You can also run tasks after a process finishes by using the `after` attribute:

```nix title="devenv.nix"
{ pkgs, ... }:

{
  # Define an application server process
  processes.app-server = {
    exec = "node server.js";
  };

  # Define a task that runs after the server stops
  tasks."app:cleanup" = {
    exec = ''
      echo "Server stopped, cleaning up..."
      rm -f ./server.pid
      rm -rf ./tmp/cache/*
    '';
    after = [ "devenv:processes:app-server" ];
  };
}
```

This ensures that cleanup tasks like removing PID files or clearing caches are executed when the application server stops.

## SDK using Task Server Protocol

See [Task Server Protocol](https://github.com/cachix/devenv/issues/1457) for a proposal how defining tasks in your favorite language would look like.
