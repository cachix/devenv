Based on [the official tutorial](https://hexdocs.pm/phoenix/installation.html).

```shell-session
$ devenv shell
$ mix local.hex --force
$ mix local.rebar --force
$ mix archive.install hex phx_new
$ mix phx.new hello
$ sed -i.bak \
    -e "s/username: \"postgres\",/username: \"$USER\",/" \
    -e "s/password: \"postgres\",/password: \"\",/" \
    ./hello/config/dev.exs && rm ./hello/config/dev.exs.bak
$ devenv up
```
