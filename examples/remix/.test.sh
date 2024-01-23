#!/usr/bin/env bash
set -ex
npm create remix -- . --template=frontsideair/remix-prisma-postgresql-template --no-install --no-git-init --no-motion --overwrite
devenv up&
timeout 20 bash -c 'until echo > /dev/tcp/localhost/5100; do sleep 0.5; done'
npx prisma migrate dev && npx prisma db seed
curl -s http://localhost:5100/ | grep "hello"