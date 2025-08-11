# Processes

Starting with a simple example:

```nix title="devenv.nix"
{ pkgs, ... }:

{
  processes = {
    silly-example.exec = "while true; do echo hello && sleep 1; done";
    ping.exec = "ping localhost";
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

A set of common services are also available, such as [services.postgres.enable](reference/options.md#servicespostgresenable) for setting up a PostgreSQL process.

## Running tasks before/after the process

Processes are automatically available as tasks, allowing you to define pre and post hooks. See the [Processes as tasks](tasks.md#processes-as-tasks) section for details on how to run tasks before a process starts or after it stops.

!!! note
    Currently, tasks are spawned per process instance. This means if you have multiple instances of a process running, tasks will run for each instance separately. See [issue #2037](https://github.com/cachix/devenv/issues/2037) for planned improvements to this behavior.
