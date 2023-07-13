#!/bin/sh
set -ex

devenv up &
DEVENV_PID=$!
trap "pkill -P $DEVENV_PID" EXIT

timeout 60 bash -c 'until MYSQL_PWD="" mysql -u root test_database < /dev/null; do sleep 0.5; done'
