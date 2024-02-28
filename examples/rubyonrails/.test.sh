#!/usr/bin/env bash
set -ex

pushd blog
    wait_for_port 5100
    rails db:create
    curl -s http://localhost:5100/ | grep "version"
popd

# make sure puma was compiled with ssl
ruby -rpuma -e 'exit 1 unless Puma.ssl?'