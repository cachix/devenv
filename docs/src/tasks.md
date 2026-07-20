# Tasks

[added-in:1.2]

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

[added-in:1.7]

You can also run all tasks in a namespace by providing just the namespace prefix:

```shell-session
$ devenv tasks run myapp
Running tasks     myapp:hello myapp:build myapp:test
Succeeded         myapp:hello           9ms
Succeeded         myapp:build         120ms
Succeeded         myapp:test          350ms
3 Succeeded                           479.14ms
```

## Dependencies between tasks

Tasks form a dependency graph (a DAG). Declare an edge between two tasks with `before` or `after`:

- `after = [ "other" ]` — run this task *after* `other` (`other` is a dependency, "upstream").
- `before = [ "other" ]` — run this task *before* `other` (`other` is "downstream" and depends on this one).

Processes are tasks too (see [Processes as tasks](#processes-as-tasks)), so the same `before`/`after` edges connect tasks and processes interchangeably — see [process dependencies](processes.md#dependencies) for process-focused examples.

`before` and `after` describe the same edge from opposite ends, so you can declare a dependency from whichever side is more convenient. These are equivalent:

```nix title="devenv.nix"
{
  # declared from the dependent task
  tasks."myapp:build".after = [ "myapp:generate" ];

  # ...is the same edge as declaring it from the dependency
  tasks."myapp:generate".before = [ "myapp:build" ];
}
```

### Dependency states

!!! tip "New in version 2.0"

A dependency waits for its target to reach a particular state before it is considered satisfied. Append an `@` suffix to choose the state explicitly:

| Suffix | Satisfied when | Failure propagates? |
| --- | --- | --- |
| `@started` | the target has begun executing | yes |
| `@ready` | a process passes its [readiness probe](processes.md#ready-probes); for oneshot tasks this means success | yes |
| `@succeeded` | the target exits with code `0` (or is skipped) | yes |
| `@completed` | the target finishes, regardless of exit code | no (soft dependency) |

When no suffix is given the default is `@ready` for processes and `@succeeded` for oneshot tasks.

A common use is running a setup task once a service is ready:

```nix title="devenv.nix"
{
  tasks."myapp:configure" = {
    exec = "create-buckets";
    after = [ "devenv:processes:garage@ready" ];
  };
}
```

## Execution modes

!!! tip "New in version 2.1"

When you run a task, devenv schedules a subgraph around it rather than only that one task. `--mode` controls how much of the graph is included:

| Mode | Runs |
| --- | --- |
| `single` | only the named task |
| `before` (default) | the task and everything *upstream* of it (its dependencies) |
| `after` | the task and everything *downstream* of it (tasks that depend on it) |
| `all` | the entire connected graph, both upstream and downstream |

```shell-session
$ devenv tasks run myapp:build               # before mode (default): build + its dependencies
$ devenv tasks run myapp:build --mode single # just build
$ devenv tasks run myapp:build --mode all    # build, its dependencies, and its dependents
```

`devenv up` starts processes in `before` mode, while `devenv test` runs in `all` mode. This difference matters for setup tasks attached to processes — see [Processes as tasks](#processes-as-tasks).

## enterShell / enterTest

`devenv:enterShell` and `devenv:enterTest` are built-in lifecycle events that run setup tasks at specific points:

- **`devenv:enterShell`** runs before the shell is entered (`devenv shell`) and before processes start (`devenv up`).
- **`devenv:enterTest`** runs before tests execute (`devenv test`).
  It depends on `devenv:enterShell`, so all shell setup tasks run first automatically.

To hook into these events, use `before` to declare that your task should run before the event completes:

```nix title="devenv.nix"
{ pkgs, lib, config, ... }:

{
  tasks = {
    "bash:hello" = {
      exec = "echo 'Hello world from bash!'";
      before = [ "devenv:enterShell" ];
    };

    "myapp:test-setup" = {
      exec = "echo 'Preparing test fixtures...'";
      before = [ "devenv:enterTest" ];
    };
  };
}
```

```shell-session
$ devenv shell
...
Running tasks     devenv:enterShell
Succeeded         devenv:git-hooks:install  25ms
Succeeded         bash:hello                 9ms
Succeeded         devenv:enterShell         13ms
3 Succeeded                                 28.14ms
```

Many devenv modules automatically hook into these events.
For example, enabling git hooks registers `devenv:git-hooks:install` as a dependency of `devenv:enterShell`.

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

You can specify a list of files to monitor with `execIfModified`. The task will only run if any of these files have been modified since the last successful run. This attribute supports glob patterns, allowing you to monitor multiple files matching specific patterns.

```nix title="devenv.nix"
{ pkgs, lib, config, ... }:

{
  tasks = {
    "myapp:build" = {
      exec = "npm run build";
      execIfModified = [
        "src/**/*.ts"  # All TypeScript files in src directory
        "*.json"       # All JSON files in the current directory
        "package.json" # Specific file
        "src"          # Entire directory
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

- `$DEVENV_TASK_INPUT`: JSON object of `tasks."myapp:mytask".input`.
- `$DEVENV_TASKS_OUTPUTS`: JSON object with dependent tasks as keys and their outputs as values.
- `$DEVENV_TASK_OUTPUT_FILE`: a writable file with tasks' outputs in JSON.
- `$DEVENV_TASK_EXPORTS_FILE`: a writable file where tasks can export environment variables. Write `name\0base64(value)\0` pairs to this file and they will be set in the environment of dependent tasks.

```nix title="devenv.nix"
{ pkgs, lib, config, ... }:

{
  tasks = {
    "myapp:mytask" = {
      exec = ''
        echo $DEVENV_TASK_INPUT > $DEVENV_ROOT/input.json
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

### Shell messages

!!! tip "New in version 2.1"

Tasks can display messages to the user when entering the shell by writing a `devenv.messages` array to `$DEVENV_TASK_OUTPUT_FILE`. This is useful for showing informational output like trace URLs or setup status after initialization.

```nix title="devenv.nix"
{ pkgs, lib, config, ... }:

{
  tasks = {
    "myapp:info" = {
      exec = ''
        echo '{"devenv":{"messages":["Setup complete. Dashboard: http://localhost:3000"]}}' > "$DEVENV_TASK_OUTPUT_FILE"
      '';
      before = [ "devenv:enterShell" ];
    };
  };
}
```

Messages are printed after the shell environment is loaded, so they remain visible in the interactive session.

### Passing inputs from the CLI

!!! tip "New in version 2.0"

You can override or add inputs when running tasks from the command line using `--input` and `--input-json`:

```shell-session
$ devenv tasks run myapp:mytask --input value=42 --input name=hello
```

Values are automatically parsed as JSON when valid, otherwise treated as strings. For example, `--input count=3` sets a number, `--input flag=true` sets a boolean, and `--input name=hello` sets a string.

You can also pass a full JSON object:

```shell-session
$ devenv tasks run myapp:mytask --input-json '{"value": 42, "name": "hello"}'
```

Both flags can be combined. `--input-json` is applied first, then individual `--input` values are merged on top (CLI wins on conflict with Nix-defined inputs).

## Processes as tasks

[added-in:1.4]

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
  tasks."app:setup-data" = {
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

You can also run tasks after a process finishes by depending on its `@completed` state (see [Dependency states](#dependency-states)). The default suffix for a process dependency is `@ready`, which fires as soon as the process is healthy, so use `@completed` to wait for it to exit instead:

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
    after = [ "devenv:processes:app-server@completed" ];
  };
}
```

This ensures that cleanup tasks like removing PID files or clearing caches are executed when the application server stops.

!!! warning "Setup tasks attached to processes and `devenv up`"

    A task that runs *after* a process — a setup or configure step wired with `processes.<name>.before = [ "devenv:<name>:configure" ]`, or equivalently `tasks."devenv:<name>:configure".after = [ "devenv:processes:<name>" ]` — is *downstream* of that process. `devenv up` schedules processes in `before` mode, which runs each process's upstream dependencies but **not** its downstream tasks, so the setup step is skipped and never runs.

    Until this is resolved ([#2852](https://github.com/cachix/devenv/issues/2852)), run `devenv up --mode all` to include downstream setup tasks. `devenv test` already runs in `all` mode, so these tasks run there. See [Execution modes](#execution-modes).

## Git Integration

[added-in:1.10]

Tasks can reference the git repository root path using `${config.git.root}`, which is particularly useful in monorepo environments:

```nix title="devenv.nix"
{ config, ... }:

{
  tasks."build:frontend" = {
    exec = "npm run build";
    cwd = "${config.git.root}/frontend";
  };

  tasks."test:backend" = {
    exec = "cargo test";
    cwd = "${config.git.root}/backend";
  };
}
```

This allows tasks to reference paths relative to the repository root regardless of where the `devenv.nix` file is located within the repository.

## SDK using Task Server Protocol

See [Task Server Protocol](https://github.com/cachix/devenv/issues/1457) for a proposal how defining tasks in your favorite language would look like.
