---
title: "honcho"
---

<!-- Do not edit this generated file. Edit docs/src/individual-docs instead. -->



[comment]: # (Please add your documentation on top of this line)

## Options

### process.managers.honcho.package



The honcho package to use.



*Type:*
package



*Default:*

```nix
pkgs.honcho
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/process-managers/honcho.nix](https://github.com/cachix/devenv/blob/main/src/modules/process-managers/honcho.nix)



### process.managers.honcho.adapter.client

Client protocol used for attach, readiness, and individual process control.



*Type:*
one of “none”, “native-api”

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/process-managers/honcho.nix](https://github.com/cachix/devenv/blob/main/src/modules/process-managers/honcho.nix)



### process.managers.honcho.adapter.stop



Adapter used to stop the running manager.



*Type:*
one of “native-api”, “command”, “process-scope”

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/process-managers/honcho.nix](https://github.com/cachix/devenv/blob/main/src/modules/process-managers/honcho.nix)



### process.managers.honcho.adapter.terminal



Terminal required by the manager launcher.



*Type:*
one of “none”, “controlling”

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/process-managers/honcho.nix](https://github.com/cachix/devenv/blob/main/src/modules/process-managers/honcho.nix)



### process.managers.honcho.capabilities.background_start



Whether the manager can remain running after the launching client exits.



*Type:*
boolean

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/process-managers/honcho.nix](https://github.com/cachix/devenv/blob/main/src/modules/process-managers/honcho.nix)



### process.managers.honcho.capabilities.cold_start_subset



Whether the manager can initially start a named subset of processes.



*Type:*
boolean

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/process-managers/honcho.nix](https://github.com/cachix/devenv/blob/main/src/modules/process-managers/honcho.nix)



### process.managers.honcho.capabilities.devenv_attach



Whether devenv can attach its interactive client to an existing manager.



*Type:*
boolean

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/process-managers/honcho.nix](https://github.com/cachix/devenv/blob/main/src/modules/process-managers/honcho.nix)



### process.managers.honcho.capabilities.individual_control



Whether devenv can start, stop, and restart individual processes through the manager.



*Type:*
boolean

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/process-managers/honcho.nix](https://github.com/cachix/devenv/blob/main/src/modules/process-managers/honcho.nix)



### process.managers.honcho.capabilities.wait_ready



Whether devenv can wait for process readiness through the manager.



*Type:*
boolean

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/process-managers/honcho.nix](https://github.com/cachix/devenv/blob/main/src/modules/process-managers/honcho.nix)
