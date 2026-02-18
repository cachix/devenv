#!/usr/bin/env bash
set -ex
devenv up&
wait_for_port 3000
curl -s http://localhost:3000/ | grep "hello world"

