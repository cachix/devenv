#!/bin/sh
set -e

echo "Startup complete..."
echo "Checking for keycloak readiness..."
echo "Process compose socket: $PC_SOCKET_PATH"
bash

for i in $(seq 1 10); do
  status=$(
    curl --silent --output /dev/null --write-out "%{http_code}" \
      "http://localhost:8089/realms/master/.well-known/openid-configuration" || true
  )

  if curl -k --head -fsS "https://localhost:9000/health/ready" ||
    [ "$status" -eq 200 ]; then
    echo "Keycloak is up and running."
    exit 0
  fi

  echo "Could not get openid-configuration for master realm. Status: $status, Try: '$i/10'."
  sleep 3
done

echo "Keycloak test failed."
exit 1
