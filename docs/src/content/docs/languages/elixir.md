---
title: "elixir"
---

<!-- Do not edit this generated file. Edit docs/src/individual-docs instead. -->

Enable Elixir with:

```nix
languages.elixir.enable = true;
```

Since devenv 2.4.1, this also provides `erl`, `erlc`, and `escript` from the Erlang runtime used to build your selected Elixir package. Selecting a different `languages.elixir.package` automatically selects its matching runtime.

For custom packages that do not expose their Erlang dependency, set `languages.elixir.erlang.package` explicitly to the runtime used to build that package.


## Choosing an Elixir version

You can pin an exact Elixir version, including release candidates, starting from 1.15.0:

```nix
languages.elixir.version = "1.17.3";
```

devenv selects the newest Erlang/OTP runtime that this Elixir version supports, and builds the Elixir language server with the same Elixir.
Versions come from [beam-overlay](https://github.com/zeeetech/beam-overlay). To use `languages.elixir.version` or `languages.elixir.versionFile`, run the following command:

`devenv inputs add beam-overlay github:zeeetech/beam-overlay --follows nixpkgs`

### Reading the version from a file

If your project already pins Elixir for asdf or mise, point devenv at that file:

```nix
languages.elixir.versionFile = ./.tool-versions;
```

devenv reads the `elixir` line, such as `elixir 1.17.3-otp-27`. A file that holds only the version, such as `.elixir-version`, works too.
The `-otp-27` part is ignored. devenv always picks the newest Erlang/OTP that the Elixir version supports.

## Mix home directory

devenv sets `MIX_HOME` to `.devenv/state/mix`. Each project gets its own Hex, Rebar, archives (like `phx_new`) and escripts.
These are built for one Elixir and Erlang/OTP version, so a shared `~/.mix` can break when projects use different versions.
Escripts installed with `mix escript.install` are on `PATH`.

To keep using a shared directory, set `env.MIX_HOME` to its full path:

```nix
env.MIX_HOME = "/home/alice/.mix";
```

## Fetching dependencies

devenv can run `mix deps.get` for you when you enter the shell:

```nix
languages.elixir.mix.deps.enable = true;
```

It installs Hex and Rebar first if they are missing. After that, it only runs again when `mix.exs`, `mix.lock`, the Elixir package or `MIX_HOME` changes, or when the `deps` directory or Hex is gone.

If your `mix.exs` is not at the root of the devenv project, set `languages.elixir.directory`:

```nix
languages.elixir.directory = "./backend";
```

[comment]: # (Please add your documentation on top of this line)

## Options

### languages.elixir.enable



Whether to enable tools for Elixir development.



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
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/elixir.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/elixir.nix)



### languages.elixir.package



Which Elixir package to use.



*Type:*
package



*Default:*

```nix
pkgs.beamPackages.elixir
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/elixir.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/elixir.nix)



### languages.elixir.directory

The Mix project’s root directory, where ` mix.exs ` lives. Defaults to the root of the devenv project.
Can be an absolute path or one relative to the root of the devenv project.



*Type:*
string



*Default:*

```nix
config.devenv.root
```



*Example:*

```nix
"./directory"
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/elixir.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/elixir.nix)



### languages.elixir.erlang.package



The Erlang runtime to include in the environment alongside Elixir.
Defaults to the runtime used to build the selected Elixir package.
Set this explicitly for custom packages that do not expose their Erlang dependency.



*Type:*
package



*Default:*

```nix
the Erlang runtime used to build languages.elixir.package
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/elixir.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/elixir.nix)



### languages.elixir.lsp.enable



Whether to enable Elixir Language Server.



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
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/elixir.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/elixir.nix)



### languages.elixir.lsp.package



The Elixir language server package to use.



*Type:*
package



*Default:*

```nix
pkgs.beamPackages.elixir-ls.override { elixir = languages.elixir.package; }
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/elixir.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/elixir.nix)



### languages.elixir.mix.deps.enable



Whether to fetch Mix dependencies with ` mix deps.get ` when entering the shell.
It only runs again when ` mix.exs `, ` mix.lock `, the Elixir package or ` MIX_HOME ` changes, or when the ` deps ` directory or Hex is missing.
Hex and Rebar are installed into ` MIX_HOME ` first if they are missing.



*Type:*
boolean



*Default:*

```nix
false
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/elixir.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/elixir.nix)



### languages.elixir.version



The Elixir version to use.
This automatically sets ` languages.elixir.package ` using [beam-overlay](https://github.com/zeeetech/beam-overlay),
together with the newest Erlang/OTP runtime that the version supports.
Elixir 1.15.0 and newer are available, including release candidates.



*Type:*
null or string



*Default:*

```nix
null
```



*Example:*

```nix
"1.17.3"
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/elixir.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/elixir.nix)



### languages.elixir.versionFile



A file to read the Elixir version from.
This can be an asdf or mise ` .tool-versions ` file with a line like ` elixir 1.17.3-otp-27 `,
or a file that holds only the version, like ` .elixir-version `.
This automatically sets ` languages.elixir.package ` using [beam-overlay](https://github.com/zeeetech/beam-overlay).
The ` -otp-NN ` suffix is ignored: devenv picks the newest Erlang/OTP that the Elixir version supports.

```nix
languages.elixir.versionFile = ./.tool-versions;
```



*Type:*
null or absolute path



*Default:*

```nix
null
```



*Example:*

```nix
./.tool-versions
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/elixir.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/elixir.nix)
