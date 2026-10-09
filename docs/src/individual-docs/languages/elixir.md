Enable Elixir with:

```nix
languages.elixir.enable = true;
```

Since devenv 2.4.1, this also provides `erl`, `erlc`, and `escript` from the Erlang runtime used to build your selected Elixir package. Selecting a different `languages.elixir.package` automatically selects its matching runtime.

For custom packages that do not expose their Erlang dependency, set `languages.elixir.erlang.package` explicitly to the runtime used to build that package.


[comment]: # (Please add your documentation on top of this line)

@AUTOGEN_OPTIONS@
