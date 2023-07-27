To ease testing of your environments, 
we provide a way to define the tests and to run them.

## Writing devenv tests

A simple test would look like:

```nix title="devenv.nix"
{ pkgs, ... }: {
  tests.basic = {
    nix = ''
      { pkgs, ... }: {
        packages = [ pkgs.ncdu ];
      }
    '';
    test = ''
      ncdu --version | grep "ncdu 2.2"
    '';
  };
}
```

```shell-session
$ devenv test
✔ Gathering tests in 0.3s.
• Found 1 test(s), running 1:
•   Testing basic ...
•     Running $ devenv ci
•     Running .test.sh.
✔   Running basic in 16.7s.
```

## Defining tests in a folder

A simple test with a test script:

```shell-session
$ ls tests/mytest/
.test.sh devenv.nix devenv.yaml
```

Define tests:

```nix title="devenv.nix"
{ config, ... }: {
  tests = config.lib.mkTests ./tests;
}
```

Run tests:

```shell-session
$ devenv test
...
```