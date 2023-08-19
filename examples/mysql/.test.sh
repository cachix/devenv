#!/usr/bin/env bash
set -ex

timeout 60 bash -c 'until MYSQL_PWD="" mysql -u root test_database < /dev/null; do sleep 0.5; done'