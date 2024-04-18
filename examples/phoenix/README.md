Based on [the official tutorial](https://hexdocs.pm/phoenix/installation.html).

```shell-session
$ devenv shell
$ mix local.hex --force
$ mix local.rebar --force
$ mix archive.install hex phx_new
$ mix phx.new --install hello
$ sed -i.bak -e "s/hostname: \"localhost\"/socket_dir: System.get_env(\"PGHOST\")/" \
    ./hello/config/dev.exs && rm ./hello/config/dev.exs.bak
$ devenv up
$ cd hello && mix ecto.create
```
