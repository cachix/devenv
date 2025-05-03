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

## SDK using Task Server Protocol

See [Task Server Protocol](https://github.com/cachix/devenv/issues/1457) for a proposal how defining tasks in your favorite language would look like.
