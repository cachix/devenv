#!/usr/bin/env bash
set -xe

wait_for_port 9000
sleep 2
clickhouse-client --query "SELECT 1"