#!/bin/sh
set -ex
mix local.hex --force
mix local.rebar --force
echo Y | mix archive.install hex phx_new
echo Y | mix phx.new hello
awk -i inplace '{
  gsub(/username: "postgres",/, "username: \"" ENVIRON["USER"] "\",")
  gsub(/password: "postgres",/, "password: \"\",")
  print
}' 'hello/config/dev.exs'
devenv up&
timeout 20 bash -c 'until echo > /dev/tcp/localhost/4000; do sleep 0.5; done'
curl -s http://localhost:4000/ | grep "Phoenix Framework"
