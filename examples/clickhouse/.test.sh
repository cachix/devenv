#!/usr/bin/env bash
set -xe 
timeout 20 bash -c 'until echo > /dev/tcp/localhost/9000; do sleep 0.5; done'
sleep 2

clickhouse-client --query "SELECT 1"