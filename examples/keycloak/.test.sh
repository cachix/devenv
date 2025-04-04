#!/bin/sh
set -ex

echo "Startup complete..."
echo "Checking for keycloak readiness..."

for i in $(seq 1 10); do
  if curl -v "http://localhost:8089/auth/realms/master/.well-known/openid-configuration"; then
    echo "Keycloak master realm up and running."
    exit 0
  fi

  echo "Could not get openid-configuration for master realm. Try: '$i/10'."
  sleep 3
done

echo "Keycloak test failed."
exit 1
