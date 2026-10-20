---
title: "ollama"
---

<!-- Do not edit this generated file. Edit docs/src/individual-docs instead. -->


[comment]: # (Please add your documentation above this line)

## Options

### services.ollama.enable

Whether to enable Ollama server for local large language models.



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
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/ollama.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/ollama.nix)



### services.ollama.package



Which package of Ollama to use.

Hardware acceleration depends on the package: use ` pkgs.ollama-cuda `,
` pkgs.ollama-rocm ` or ` pkgs.ollama-vulkan ` for GPU support on Linux.
On macOS, ` pkgs.ollama ` uses Metal.



*Type:*
package



*Default:*

```nix
pkgs.ollama
```



*Example:*

```nix
pkgs.ollama-cuda
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/ollama.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/ollama.nix)



### services.ollama.environmentVariables



Additional environment variables for the Ollama server process.
See ` ollama serve --help ` for available options.



*Type:*
attribute set of string



*Default:*

```nix
{ }
```



*Example:*

```nix
{
  OLLAMA_CONTEXT_LENGTH = "8192";
  OLLAMA_KEEP_ALIVE = "10m";
}
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/ollama.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/ollama.nix)



### services.ollama.host



The host address the Ollama HTTP server listens on.
Wrap IPv6 addresses in brackets.



*Type:*
string



*Default:*

```nix
"127.0.0.1"
```



*Example:*

```nix
"[::]"
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/ollama.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/ollama.nix)



### services.ollama.loadModels



Models to download with ` ollama pull ` once the server is ready.
Models that are already present are not downloaded again.

Search for models at [https://ollama.com/library](https://ollama.com/library).



*Type:*
list of string



*Default:*

```nix
[ ]
```



*Example:*

```nix
[
  "gemma3"
  "qwen3:0.6b"
]
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/ollama.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/ollama.nix)



### services.ollama.modelsDir



The directory Ollama reads models from and downloads new models to.

Point this at a shared location (such as ` ~/.ollama/models `, as an absolute path)
to reuse models across projects instead of downloading them per project.



*Type:*
string



*Default:*

```nix
"${config.devenv.state}/ollama/models"
```



*Example:*

```nix
"/home/user/.ollama/models"
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/ollama.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/ollama.nix)



### services.ollama.port



The base port the Ollama HTTP server listens on.
The actual port is allocated by devenv and may differ;
` OLLAMA_HOST ` and ` OLLAMA_BASE_URL ` point to it.



*Type:*
16 bit unsigned integer; between 0 and 65535 (both inclusive)



*Default:*

```nix
11434
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/ollama.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/ollama.nix)
