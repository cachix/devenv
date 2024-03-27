# Tests

Tests are a way to ensure that your development environment is working as expected.

Running `devenv test` will build your environment and run the tests defined in `enterTest`.

If you have [processes](/processes.md) defined in your environment, they will be started and stopped for you.

## Writing your first test

A simple test would look like:

```nix title="devenv.nix"
{ pkgs, ... }: {
  packages = [ pkgs.ncdu ];

  enterTest = ''
    ncdu --version | grep "ncdu 2.2"
  '';
}
```

```shell-session
$ devenv test
✔ Building tests in 2.5s.
• Running tests ...
Setting up shell environment...
Running test...
ncdu 2.2
✔ Running tests in 4.7s.
✔ Tests passed. in 0.0s.
```

By default, the `enterTest` detects if `.test.sh` file exists and runs it.

## Testing with processes

If you have [processes](/processes.md) defined in your environment,
they will be started and stopped for you.

```nix title="devenv.nix"
{ pkgs, ... }: {
  services.nginx = {
    enable = true;
    httpConfig = ''
      server {
        listen 8080;
        location / {
          return 200 "Hello, world!";
        }
      }
    '';
  };

  enterTest = ''
    wait_for_port 8080
    curl -s localhost:8080 | grep "Hello, world!"
  '';
}
```

```shell-session
$ devenv test
✔ Building tests in 2.5s.
✔ Building processes in 15.7s.
• Starting processes ...• PID is 113105
• See logs:  $ tail -f /run/user/1000/nix-shell.upTad4/.tmpv25BxA/processes.log
• Stop:      $ devenv processes stop
✔ Starting processes in 0.0s.
• Running tests ...
Setting up shell environment...
Running test...
ncdu 2.2
✔ Running tests in 4.7s.
• Stopping process with PID 113105
✔ Tests passed. in 0.0s.
```

## Provided functions for enterTest

- `wait_for_port <port> <timeout>`: waits for a port to be open

If you'd like more functions to be added, take a look at [NixOS tests](https://nixos.org/manual/nixos/stable/#sec-nixos-tests)
and open an issue for what you need.