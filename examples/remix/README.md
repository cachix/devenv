```shell-session
$ npm create remix -- . --template=frontsideair/remix-prisma-postgresql-template --install --no-git-init --no-motion --overwrite
$ devenv up
$ npx prisma migrate dev
$ npx prisma db seed
```
