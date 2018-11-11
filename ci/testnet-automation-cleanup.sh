#!/usr/bin/env bash
set -e

cd "$(dirname "$0")/.."

echo --- find testnet configuration
net/gce.sh config -p testnet-automation

echo --- delete testnet
net/gce.sh delete -p testnet-automation
