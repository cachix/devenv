Tasks allow you to form dependencies between commands, executed in parallel.

## Defining tasks

```nix title="devenv.nix"
{ pkgs, ... }:

{
  tasks."myapp:hello" = {
    exec = ''echo "Hello, world!"'';
    desc = "hello world in bash";
  };
}
```

```shell-session
$ devenv tasks run hello
• Building shell ...
• Entering shell ...
Hello, world!
$
```

## Using your favourite language

Tasks can also reference scripts and depend on other tasks, for example when entering the shell:

```nix title="devenv.nix"
{ pkgs, lib, config, ... }:

{
  tasks = {
    "python:hello"" = {
      exec = ''print("Hello world from Python!")'';
      package = config.languages.python.package;
    };
    "bash:hello" = {
      exec = "echo 'Hello world from bash!'";
      depends = [ "python:hello" ];
    };
    "devenv:enterShell".depends = [ "bash:hello" ];
  };
}
```

```shell-session
$ devenv shell
• Building shell ...
• Entering shell ...
...
$
```


`status`
