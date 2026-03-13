#!/bin/sh
set -e

# TODO: Realm export tests were removed because the H2 embedded database
# (dev-file) holds a file lock that isn't reliably released by the time the
# export JVM starts. Consider re-adding export tests with a PostgreSQL backend.

echo "Waiting for keycloak readiness..."
wait_for_processes
echo "Keycloak is healthy."
