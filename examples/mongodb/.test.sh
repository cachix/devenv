#!/bin/sh
set -x

mongosh --version
mongod --version

check_mongo_status() {
    echo "Waiting for service to become available..."
    MONGO_OUTPUT=$(mongosh --quiet --eval "{ ping: 1 }" 2>&1)
    MONGO_EXIT_STATUS=$?
}

check_if_mongo_user_created() {
    # This line queries mongo using the shell and trims the output to make sure
    # it is either an empty string or the created user document
    createdUser=$(echo "use admin\n db.system.users.find({ user: \"mongouser\", db: \"admin\", \"roles.role\": \"root\", \"roles.db\": \"admin\" })" | mongosh --quiet --eval --shell | tail -n +2 | sed 's/^admin> //')

    if [ -n "$createdUser" ]; then
       MONGO_EXIT_STATUS=0
    else
       MONGO_EXIT_STATUS=1
    fi
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

