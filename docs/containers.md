!!! info "New in version 0.6."

!!! note

    To be able to generate containers on macOS, you will need to use a remote Linux builder.

    The easiest ways to do so are:
    - [Set the remote builder up using Nix](https://nixos.org/manual/nixpkgs/unstable/#sec-darwin-builder).
    - [Use the nix-darwin linux-builder module](https://github.com/LnL7/nix-darwin/blob/master/modules/nix/linux-builder.nix).

Use `devenv container build <name>` to generate an [OCI container](https://opencontainers.org/) from your development environment.

By default, `shell` and `processes` containers are predefined. You can also [craft your own](#running-artifacts)!

Examples of what `devenv container` can do:

- `devenv container build shell`: Generate a container and [start the environment](#entering-the-development-environment), equivalent of using `devenv shell`.
- `devenv container build processes`: Generate a container and [start processes](#running-processes), equivalent of using `devenv up`.
- `devenv container --registry docker://ghcr.io/ copy <name>`: [Copy the container](#copying-container-to-a-registry) `<name>` into the **GitHub package registry**.
- `devenv container run <name>`: Run the container `<name>` using **Docker**.

## Entering the development environment

Given a simple environment, using Python:

```nix title="devenv.nix"
{
  name = "simple-python-app";

  languages.python.enable = true;
}
```

Generate a container specification that enters the environment:

```shell-session
$ devenv container build shell
/nix/store/...-image-devenv.json
```

Let's test it locally using Docker:

```shell-session
$ devenv container run shell
...
(devenv) bash-5.2# python
Python 3.10.9 (main, Dec  6 2022, 18:44:57) [GCC 12.2.0] on linux
Type "help", "copyright", "credits" or "license" for more information.
>>> 
```

## Running processes

A common deployment strategy is to run each [process](./processes.md) as an entrypoint to the container.

```nix title="devenv.nix"
{
  name = "myapp";

  packages = [ pkgs.procps ];

  processes.hello-docker.exec = "while true; do echo 'Hello Docker!' && sleep 1; done";
  processes.hello-nix.exec = "while true; do echo 'Hello Nix!' && sleep 1; done";

  # Exclude the source repo to make the container smaller.
  containers."processes".copyToRoot = null; 
}
```

You can now copy the newly created image and start the container:

```shell-session
$ devenv container run processes
...
06:30:06 system         | hello-docker.1 started (pid=15)
06:30:06 hello-docker.1 | Hello Docker!
06:30:06 system         | hello-nix.1 started (pid=16)
06:30:06 hello-nix.1    | Hello Nix!
06:30:07 hello-nix.1    | Hello Nix!
06:30:07 hello-docker.1 | Hello Docker!
06:30:08 hello-nix.1    | Hello Nix!
06:30:08 hello-docker.1 | Hello Docker!
```

## Running a single process


You can specify the command to run when the container starts (instead of entering the default development environment):

```nix title="devenv.nix"
{
  processes.serve.exec = "python -m http.server";

  containers."serve".name = "myapp";
  containers."serve".startupCommand = config.processes.serve.exec;
}
```

```shell-session
$ devenv container run serve
```

## Running artifacts

If you're building binaries as part of the development environment, you can choose to only include those in the final image:

```nix title="devenv.nix"
{
  # watch local changes and build the project to ./dist
  processes.build.exec = "${pkgs.watchexec}/bin/watchexec my-build-tool";
  
  containers."prod".copyToRoot = ./dist;
  containers."prod".startupCommand = "/mybinary serve";
}
```

```shell-session
$ devenv container run prod
...
```



## Copying a container to a registry

To copy a container into a registry use `copy` subcommand:

```shell-session
$ devenv container --registry docker:// copy processes
```

Another common example is deploying to [fly.io](https://fly.io). 
Any arguments passed to `--copy-args` are forwarded to [skopeo copy](https://github.com/containers/skopeo/blob/main/docs/skopeo-copy.1.md#options):


```shell-session
$ devenv container --registry docker://registry.fly.io/ --copy-args="--dest-creds x:$(flyctl auth token)" copy processes
```

You can also specify these options declaratively:

```nix title="devenv.nix"
{
  containers."processes".registry = "docker://registry.fly.io/";
  containers."processes".defaultCopyArgs = [
    "--dest-creds"
    "x:\"$(${pkgs.flyctl}/bin/flyctl auth token)\""
  ];
}
```

See this [fly.io example](https://github.com/cachix/devenv/tree/main/examples/fly.io) for how to get started.

## Changing the environment based on the build type

If you want to provide the `openssl` package to native and container environments, but `git` only for native environments:

```nix title="devenv.nix"
{ pkgs, config, lib, ... }:

{
  packages = [ pkgs.openssl ] 
    ++ lib.optionals (!config.container.isBuilding) [ pkgs.git ];
}
```

You can also conditionalize based on the particular container that is being built, for example, `config.containers."processes".isBuilding`.
