#!/usr/bin/env bash
set -ex

pushd blog
  rails db:create
popd

wait_for_port 3000
curl -s http://localhost:3000/ | grep "version"

# make sure puma was compiled with ssl
ruby -rpuma -e 'exit 1 unless Puma.ssl?'
