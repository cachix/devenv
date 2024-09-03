Tasks allow you to form dependencies between commands, executed in parallel.

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
$ devenv tasks run hello
Hello, world
$
```

## enterShell / enterTest

If you'd like the tasks to run as part of the `enterShell` or `enterTest`:

```nix title="devenv.nix"
{ pkgs, lib, config, ... }:

{
  tasks = {
    "bash:hello".exec = "echo 'Hello world from bash!'";
    "devenv:enterShell".depends = [ "bash:hello" ];
    "devenv:enterTest".depends = [ "bash:hello" ];
  };
}
```

## Using your favourite language

Tasks can also reference scripts and depend on other tasks, for example when entering the shell:

```nix title="devenv.nix"
{ pkgs, lib, config, ... }:

{
  tasks = {
    "python:hello"" = {
      exec = ''
        print("Hello world from Python!")
      '';
      package = config.languages.python.package;
    };
  };
}
```

```shell-session
$ devenv shell
...
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

- `$DEVENV_TASK_INPUTS`: JSON object serializing `tasks."myapp:mytask".inputs`.
- `$DEVENV_TASK_OUTPUTS`: a writable file with tasks' outputs in JSON.
- `$DEVENV_TASKS_OUTPUTS`: JSON object with dependent tasks as keys and their outputs as values.

```nix title="devenv.nix"
{ pkgs, lib, config, ... }:

{
  tasks = {
    "myapp:mytask" = {
      exec = ''
        echo $DEVENV_TASK_INPUTS > $DEVENV_ROOT/inputs.json
        echo '{ "output" = 1; }' > $DEVENV_TASK_OUTPUTS
        echo $DEVENV_TASKS_OUTPUTS > $DEVENV_ROOT/outputs.json
      '';
      inputs = {
        value = 1;
      };
    };
  };
}
```

## SDK

See [xxx](xxx) for a proposal how defining tasks in your favorite language would look like.
