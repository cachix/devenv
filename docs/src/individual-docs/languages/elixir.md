Enable Elixir with:

```nix
languages.elixir.enable = true;
```

Since devenv 2.4.1, this also provides `erl`, `erlc`, and `escript` from the Erlang runtime used to build your selected Elixir package. Selecting a different `languages.elixir.package` automatically selects its matching runtime.

For custom packages that do not expose their Erlang dependency, set `languages.elixir.erlang.package` explicitly to the runtime used to build that package.


## Choosing an Elixir version

Since devenv 2.4.1, you can pin an exact Elixir version, including release candidates, starting from 1.15.0:

```nix
languages.elixir.version = "1.17.3";
```

devenv selects the newest Erlang/OTP runtime that this Elixir version supports, and builds the Elixir language server with the same Elixir.
Versions come from [beam-overlay](https://github.com/zeeetech/beam-overlay). To use `languages.elixir.version`, run the following command:

`devenv inputs add beam-overlay github:zeeetech/beam-overlay --follows nixpkgs`

[comment]: # (Please add your documentation on top of this line)

@AUTOGEN_OPTIONS@
