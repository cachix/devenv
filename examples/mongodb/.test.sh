#!/bin/sh
set -ex

. "$DEVENV_TEST_LIB"

mongosh --version
mongod --version

mongo_user_exists() {
    # Trim the shell's own output so that the result is either empty or the
    # created user document.
    created=$(echo "use admin\n db.system.users.find({ user: \"mongouser\", db: \"admin\", \"roles.role\": \"root\", \"roles.db\": \"admin\" })" | mongosh --quiet --eval --shell | tail -n +2 | sed 's/^admin> //')
    [ -n "$created" ]
}

wait_until 30 mongosh --quiet --eval "{ ping: 1 }"
wait_until 10 mongo_user_exists
