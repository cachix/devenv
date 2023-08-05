#!/bin/sh
set -ex
devenv up&
timeout 20 bash -c 'until psql -c "SELECT 1" mydb; do sleep 0.5; done'
