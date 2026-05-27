#!/bin/sh
set -x

mongosh --version
mongod --version

# Since MONGODB_PORT is the port of the first node (primary / node1)
PORT=${MONGODB_PORT:-27017}

check_mongo_status() {
    echo "Waiting for replica set to be initiated..."
    MONGO_OUTPUT=$(mongosh --port "$PORT" -u mongouser -p secret --authenticationDatabase admin --quiet --eval "rs.status().ok" 2>&1)
    MONGO_EXIT_STATUS=$?
}

# Allow some time for processes to start up and election to complete
sleep 15

for i in $(seq 1 15); do
    check_mongo_status
    if [ $MONGO_EXIT_STATUS -eq 0 ] && [ "$MONGO_OUTPUT" = "1" ]; then
        echo "Replica set is up and authenticated successfully!"
        break
    else
        echo "Still waiting (output: $MONGO_OUTPUT, status: $MONGO_EXIT_STATUS)..."
        sleep 2
    fi
done

if [ $MONGO_EXIT_STATUS -ne 0 ]; then
    echo "Replica set check failed!"
    exit 1
fi

# Check that we have 3 members in the replica set
MEMBERS_COUNT=$(mongosh --port "$PORT" -u mongouser -p secret --authenticationDatabase admin --quiet --eval "rs.status().members.length" | tr -d '\r\n ')
echo "Number of replica set members: $MEMBERS_COUNT"
if [ "$MEMBERS_COUNT" != "3" ]; then
    echo "Error: expected 3 members, got '$MEMBERS_COUNT'"
    exit 1
fi

echo "All checks passed successfully!"
exit 0
