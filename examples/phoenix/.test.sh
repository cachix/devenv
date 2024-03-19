#!/usr/bin/env bash
set -ex

pushd hello
    wait_for_port 4000
    mix ecto.create
    curl -s http://localhost:4000/ | grep "Phoenix Framework"
popd
