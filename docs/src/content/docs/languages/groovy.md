---
title: "groovy"
---

<!-- Do not edit this generated file. Edit docs/src/individual-docs instead. -->



[comment]: # "Please add your documentation on top of this line"

## Options

### languages.groovy.enable

Whether to enable tools for Groovy development.



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
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/groovy.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/groovy.nix)



### languages.groovy.package



The Groovy package to use.



*Type:*
package



*Default:*

```nix
pkgs.groovy
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/groovy.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/groovy.nix)



### languages.groovy.lsp.enable



Whether to enable Groovy Language Server.



*Type:*
boolean



*Default:*

```nix
true
```



*Example:*

```nix
true
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/groovy.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/groovy.nix)



### languages.groovy.lsp.package



The Groovy language server package to use.



*Type:*
package



*Default:*

```nix
pkgs.groovy-language-server
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/groovy.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/groovy.nix)
