# Processes

Devenv uses [process-compose](https://github.com/F1bonacc1/process-compose) to manage and orchestrate processes in your development environment. Process-compose provides process supervision, dependency management, health checks, and a TUI interface for monitoring your processes.

## Basic example

```nix title="devenv.nix"
{ pkgs, ... }:

{
  processes = {
    silly-example.exec = "while true; do echo hello && sleep 1; done";
    ping.exec = "ping localhost";
    # Process that runs in a specific directory
    server = {
      exec = "python -m http.server";
      cwd = "./public";
    };
  };
}
```

To start the processes in the foreground, run:

```shell-session

$ devenv up
Starting processes ...

20:37:44 system          | ping.1 started (pid=4094686)
20:37:44 system          | silly-example.1 started (pid=4094688)
20:37:44 silly-example.1 | hello
20:37:44 ping.1          | PING localhost (127.0.0.1) 56 bytes of data.
20:37:44 ping.1          | 64 bytes from 127.0.0.1: icmp_seq=0 ttl=64 time=0.127 ms
20:37:45 silly-example.1 | hello
20:37:45 ping.1          | 64 bytes from 127.0.0.1: icmp_seq=1 ttl=64 time=0.257 ms
20:37:46 silly-example.1 | hello
20:37:46 ping.1          | 64 bytes from 127.0.0.1: icmp_seq=2 ttl=64 time=0.242 ms
20:37:47 silly-example.1 | hello
20:37:47 ping.1          | 64 bytes from 127.0.0.1: icmp_seq=3 ttl=64 time=0.249 ms
...
```

## How process-compose works

When you run `devenv up`, devenv generates a process-compose configuration file that:

1. **Spawns and supervises processes**: Each process defined in `devenv.nix` becomes a managed process that process-compose monitors and can restart if needed
2. **Provides a TUI interface**: You can interact with processes, view logs, restart individual processes, and navigate between them
3. **Handles dependencies**: Processes can depend on each other and start in the correct order
4. **Manages health checks**: Processes can define health checks to ensure they're ready before dependent processes start
5. **Logs output**: All process output is captured and available in the TUI and in log files at `$DEVENV_STATE/process-compose/`

## Using pre-built services

Devenv provides many pre-configured services that are already set up with proper process management. See the [Services documentation](services.md) for a complete list of available services like:

- [PostgreSQL](supported-services/postgres.md)
- [Redis](supported-services/redis.md)
- [MySQL](supported-services/mysql.md)
- [MongoDB](supported-services/mongodb.md)
- [Elasticsearch](supported-services/elasticsearch.md)
- And many more...

These services come with sensible defaults, health checks, and proper initialization scripts.

## Git Integration

!!! tip "New in version 1.10"

Processes can reference the git repository root path using `${config.git.root}`, which is particularly useful in monorepo environments:

```nix title="devenv.nix"
{ config, ... }:

{
  processes.frontend = {
    exec = "npm run dev";
    cwd = "${config.git.root}/frontend";
  };

  processes.backend = {
    exec = "cargo run";
    cwd = "${config.git.root}/backend";
  };
}
```

This allows processes to reference paths relative to the repository root regardless of where the `devenv.nix` file is located within the repository.

## Running tasks before/after the process

Processes are automatically available as tasks, allowing you to define pre and post hooks. See the [Processes as tasks](tasks.md#processes-as-tasks) section for details on how to run tasks before a process starts or after it stops.

!!! note
    Currently, tasks are spawned per process instance. This means if you have multiple instances of a process running, tasks will run for each instance separately. See [issue #2037](https://github.com/cachix/devenv/issues/2037) for planned improvements to this behavior.
