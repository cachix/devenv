#!/bin/sh
set -x

mongosh --version
mongod --version

check_mongo_status() {
    echo "Waiting for service to become available..."
    local PORT_ARG=""
    if [ -n "$MONGODB_PORT" ]; then
        PORT_ARG="--port $MONGODB_PORT"
    fi
    MONGO_OUTPUT=$(mongosh $PORT_ARG --quiet --eval "{ ping: 1 }" 2>&1)
    MONGO_EXIT_STATUS=$?
}

check_if_mongo_user_created() {
    # Verify we can authenticate with the created user
    local PORT_ARG=""
    if [ -n "$MONGODB_PORT" ]; then
        PORT_ARG="--port $MONGODB_PORT"
    fi
    mongosh $PORT_ARG -u mongouser -p secret --authenticationDatabase admin --quiet --eval "db.runCommand({ connectionStatus: 1 })"
    MONGO_EXIT_STATUS=$?
}

# Just to allow the service some time to start up 
sleep 10

for i in $(seq 1 10); do
    check_mongo_status
    if [ $MONGO_EXIT_STATUS -eq 0 ]; then
        echo "Service is up..."
        break
    else
        sleep 1
    fi
done

echo "Startup complete..."
echo "Checking for initial user creation..."
for i in $(seq 1 10); do
    check_if_mongo_user_created
    if [ $MONGO_EXIT_STATUS -eq 0 ]; then
        echo "Initial user created..."
        break
    else
        sleep 1
    fi
done

# Exit the script
exit $MONGO_EXIT_STATUS

