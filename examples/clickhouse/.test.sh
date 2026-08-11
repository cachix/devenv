#!/usr/bin/env bash
set -xe

CLICKHOUSE_PORT=${CLICKHOUSE_PORT:?CLICKHOUSE_PORT is not set}

wait_for_port "$CLICKHOUSE_PORT"
clickhouse-client --host 127.0.0.1 --port "$CLICKHOUSE_PORT" --query "SELECT 1"
