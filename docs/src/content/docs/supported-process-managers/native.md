---
title: "native"
---

<!-- Do not edit this generated file. Edit docs/src/individual-docs instead. -->


The native process manager is built into devenv, enabled by default, and is the
recommended way to run processes. See the [Processes guide](/processes/) for
configuration, dependencies, readiness probes, restart policies, socket
activation, file watching, and automatic port allocation.

[comment]: # (Please add your documentation above this line)

## Options

### process.managers.native.adapter.client

Client protocol used for attach, readiness, and individual process control.



*Type:*
one of “none”, “native-api”

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/process-managers/native.nix](https://github.com/cachix/devenv/blob/main/src/modules/process-managers/native.nix)



### process.managers.native.adapter.stop



Adapter used to stop the running manager.



*Type:*
one of “native-api”, “command”, “process-scope”

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/process-managers/native.nix](https://github.com/cachix/devenv/blob/main/src/modules/process-managers/native.nix)



### process.managers.native.adapter.terminal



Terminal required by the manager launcher.



*Type:*
one of “none”, “controlling”

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/process-managers/native.nix](https://github.com/cachix/devenv/blob/main/src/modules/process-managers/native.nix)



### process.managers.native.capabilities.background_start



Whether the manager can remain running after the launching client exits.



*Type:*
boolean

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/process-managers/native.nix](https://github.com/cachix/devenv/blob/main/src/modules/process-managers/native.nix)



### process.managers.native.capabilities.cold_start_subset



Whether the manager can initially start a named subset of processes.



*Type:*
boolean

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/process-managers/native.nix](https://github.com/cachix/devenv/blob/main/src/modules/process-managers/native.nix)



### process.managers.native.capabilities.devenv_attach



Whether devenv can attach its interactive client to an existing manager.



*Type:*
boolean

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/process-managers/native.nix](https://github.com/cachix/devenv/blob/main/src/modules/process-managers/native.nix)



### process.managers.native.capabilities.individual_control



Whether devenv can start, stop, and restart individual processes through the manager.



*Type:*
boolean

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/process-managers/native.nix](https://github.com/cachix/devenv/blob/main/src/modules/process-managers/native.nix)



### process.managers.native.capabilities.wait_ready



Whether devenv can wait for process readiness through the manager.



*Type:*
boolean

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/process-managers/native.nix](https://github.com/cachix/devenv/blob/main/src/modules/process-managers/native.nix)



### process.managers.native.logs



Default stdout/stderr log retention for native-managed processes.



*Type:*
submodule



*Default:*

```nix
{ }
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/process-managers/native.nix](https://github.com/cachix/devenv/blob/main/src/modules/process-managers/native.nix)



### process.managers.native.logs.maxFileSize



Maximum bytes per retained stdout/stderr log file. Only applies to the native process manager.



*Type:*
positive integer, meaning >0



*Default:*

```nix
1048576
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/process-managers/native.nix](https://github.com/cachix/devenv/blob/main/src/modules/process-managers/native.nix)



### process.managers.native.logs.maxFiles



Number of retained files per stream, including the active file. Only applies to the native process manager.



*Type:*
positive integer, meaning >0



*Default:*

```nix
2
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/process-managers/native.nix](https://github.com/cachix/devenv/blob/main/src/modules/process-managers/native.nix)
