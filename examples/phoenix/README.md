Based on [the official tutorial](https://hexdocs.pm/phoenix/installation.html).

```shell-session
$ devenv shell
$ mix local.hex --force
$ mix local.rebar --force
$ mix archive.install hex phx_new
$ mix phx.new hello
$ awk -i inplace '{
    gsub(/username: "postgres",/, "username: \"" ENVIRON["USER"] "\",")
    gsub(/password: "postgres",/, "password: \"\",")
    print
  }' 'hello/config/dev.exs'
$ devenv up
```
