#!/usr/bin/env bash
set -ex

mix local.hex --force
mix local.rebar --force
echo Y | mix archive.install hex phx_new
echo Y | mix phx.new hello
sed -i.bak -e "s/username: \"postgres\",/socket_dir: System.get_env(\"PGDATA\"),/" \
    ./hello/config/dev.exs && rm ./hello/config/dev.exs.bak
    
wait_for_port 4000
curl -s http://localhost:4000/ | grep "Phoenix Framework"
