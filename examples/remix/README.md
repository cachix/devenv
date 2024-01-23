```shell-session
$ npm create remix -- . --template=frontsideair/remix-prisma-postgresql-template --no-install --no-git-init --no-motion --overwrite
$ devenv shell
$ devenv up
$ npx prisma migrate dev
$ npx prisma db seed
```
