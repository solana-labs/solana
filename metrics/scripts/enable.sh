#!/usr/bin/env bash

export INFLUX_HOST=http://localhost:8086
export INFLUX_DATABASE=local
export INFLUX_USERNAME=admin
export INFLUX_PASSWORD=admin

export SOLANA_METRICS_CONFIG="host=$INFLUX_HOST,db=$INFLUX_DATABASE,u=$INFLUX_USERNAME,p=$INFLUX_PASSWORD"

echo Local metrics enabled
