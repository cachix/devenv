set -e

kafka-topics.sh --list --bootstrap-server localhost:$KAFKA_PORT
