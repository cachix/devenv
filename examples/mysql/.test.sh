#!/usr/bin/env bash
set -ex

. "$DEVENV_TEST_LIB"

export MYSQL_PWD=""

wait_until 60 mariadb -u root test_database < /dev/null
