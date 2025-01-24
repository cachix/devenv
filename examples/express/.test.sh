#!/usr/bin/env bash
set -ex
devenv up&
timeout 20 bash -c 'until echo > /dev/tcp/localhost/3000; do sleep 0.5; done'
curl -s http://localhost:3000/ | grep "hello world"

