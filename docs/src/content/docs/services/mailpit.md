---
title: "mailpit"
---

<!-- Do not edit this generated file. Edit docs/src/individual-docs instead. -->



[comment]: # (Please add your documentation on top of this line)

## Options

### services.mailpit.enable



Whether to enable mailpit process.



*Type:*
boolean



*Default:*

```nix
false
```



*Example:*

```nix
true
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/mailpit.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mailpit.nix)



### services.mailpit.package



Which package of mailpit to use



*Type:*
package



*Default:*

```nix
pkgs.mailpit
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/mailpit.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mailpit.nix)



### services.mailpit.additionalArgs

Additional arguments passed to ` mailpit `.



*Type:*
list of strings concatenated with “\\n”



*Default:*

```nix
[ ]
```



*Example:*

```nix
[
  "--max=500"
]
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/mailpit.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mailpit.nix)



### services.mailpit.smtpListenAddress



Listen address for SMTP.

Either ` <host>:<port> `, where the port is the base devenv allocates
from, or a unix socket as ` unix:<path>:<permissions> `, which is passed
to mailpit as given. Mailpit requires the permissions.



*Type:*
string



*Default:*

```nix
"127.0.0.1:1025"
```



*Example:*

```nix
"unix:${config.env.DEVENV_RUNTIME}/mailpit-smtp.sock:660"
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/mailpit.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mailpit.nix)



### services.mailpit.uiListenAddress



Listen address for UI.

Either ` <host>:<port> `, where the port is the base devenv allocates
from, or a unix socket as ` unix:<path>:<permissions> `, which is passed
to mailpit as given. Mailpit requires the permissions.



*Type:*
string



*Default:*

```nix
"127.0.0.1:8025"
```



*Example:*

```nix
"unix:${config.env.DEVENV_RUNTIME}/mailpit-ui.sock:660"
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/mailpit.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mailpit.nix)
