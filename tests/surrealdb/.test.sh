set -e

wait_for_port 8080

surreal --is-ready -e $SURREAL_ENDPOINT
