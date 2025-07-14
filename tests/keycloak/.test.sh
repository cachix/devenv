#!/bin/sh
set -e

echo "Startup complete..."
echo "Checking for keycloak readiness..."
echo "Process compose socket: $PC_SOCKET_PATH"
# bash

test_connection() {
  for i in $(seq 1 10); do
    if curl -k --head -fsS "https://localhost:9000/health/ready"; then
      echo "Keycloak is up and running."
      return 0
    fi

    echo "Could not check health endpoint on keycloak or not ready yet, Try: '$i/10'."
    sleep 3
  done

  echo "!! Keycloak test failed."
  return 1
}

test_export() {
  echo "Stop keycloak..."
  process-compose process stop keycloak -u "$PC_SOCKET_PATH"

  for i in $(seq 1 10); do
    if
      [ "$(
        process-compose process get keycloak -o json -u "$PC_SOCKET_PATH" |
          jq -r ".[0].status"
      )" = "Completed" ]
    then
      completed="true"
      break
    fi

    sleep 2
  done

  old_timestamp=$(stat -c %Y "./realms/test.json")

  echo "Export realms..."
  process-compose process start keycloak-realm-export-all -u "$PC_SOCKET_PATH"

  completed="false"
  for i in $(seq 1 30); do
    if
      [ "$(
        process-compose process get keycloak-realm-export-all \
          -o json -u "$PC_SOCKET_PATH" |
          jq -r ".[0].status"
      )" = "Completed" ]
    then
      completed="true"
      break
    fi

    sleep 2
  done

  if [ "$completed" != "true" ]; then
    echo "!! Realm export did not complete in time."
    return 1
  fi

  new_timestamp=$(stat -c %Y "./realms/test.json")
  if ! [ "$new_timestamp" -gt "$old_timestamp" ]; then
    echo "!! Realm 'test' did not get exported (was not modified)."
    return 1
  fi
}

test_connection
test_export
