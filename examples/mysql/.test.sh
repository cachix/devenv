#!/usr/bin/env bash
set -ex

devenv up &
DEVENV_PID=$!
export DEVENV_PID

devenv_stop() {
    pkill -P "$DEVENV_PID"
}

trap devenv_stop EXIT

timeout 60 bash -c 'until MYSQL_PWD="" mysql -u root test_database < /dev/null; do sleep 0.5; done'
