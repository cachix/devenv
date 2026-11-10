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

@AUTOGEN_OPTIONS@
