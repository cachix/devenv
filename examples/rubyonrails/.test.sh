#!/usr/bin/env bash
set -ex
rails new blog -d=postgresql
devenv up&
timeout 20 bash -c 'until echo > /dev/tcp/localhost/5100; do sleep 0.5; done'
(cd blog && rails db:create)
curl -s http://localhost:5100/ | grep "version"