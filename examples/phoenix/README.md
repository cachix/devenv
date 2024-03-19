Based on [the official tutorial](https://hexdocs.pm/phoenix/installation.html).

```shell-session
$ devenv shell
$ mix local.hex --force
$ mix local.rebar --force
$ mix archive.install hex phx_new
$ mix phx.new hello
$ devenv up
$ cd hello && mix ecto.create
```
