#!/usr/bin/env bash
set -ex

pushd blog
    wait_for_port 3000
    rails db:create
    curl -s http://localhost:3000/ | grep "version"
popd

# make sure puma was compiled with ssl
ruby -rpuma -e 'exit 1 unless Puma.ssl?'