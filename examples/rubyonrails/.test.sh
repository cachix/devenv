#!/usr/bin/env bash
set -ex

pushd blog
    timeout 20 bash -c 'until echo > /dev/tcp/localhost/5100; do sleep 0.5; done'
    rails db:create
    curl -s http://localhost:5100/ | grep "version"
popd

# make sure puma was compiled with ssl
ruby -rpuma -e 'exit 1 unless Puma.ssl?'